// SPDX-License-Identifier: MIT AND Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use xshell::Shell;

use crate::environment::{
    cargo_cmd, get_workspace_packages, get_workspace_root, CmdExt, Package, PackageManifest,
    ProgressGuard,
};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Cargo tree arguments for duplicate dependency detection.
const CARGO_TREE_ARGS: &[&str] = &[
    "tree",
    "--target=all",
    "--all-features",
    "--duplicates",
    // Keeps full tree so we can analyze internal workspace memberships.
    "--no-dedupe",
    // Filter out dependencies which are not exposed to external consumers.
    "--edges",
    "no-build",
    "--edges",
    "no-dev",
    "--prefix",
    "depth",
];

/// Custom error type for lint failures with detailed information.
#[derive(Debug)]
enum LintError {
    /// Duplicate dependencies found in package dependency tree.
    DuplicateDependencies(Vec<(String, String)>), // (package_name, tree_output)
    /// Stale entries in `allowed_duplicates` configuration.
    StaleAllowedDuplicates(Vec<(String, Vec<String>)>), // (package_name, stale_entries)
    /// Deprecated MSRV settings found in clippy.toml files.
    DeprecatedClippyMsrv(Vec<String>), // file_paths
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateDependencies(duplicates) => {
                write!(f, "Error: Found duplicate dependencies")?;
                for (pkg_name, output) in duplicates {
                    write!(f, "\n  {}: {}", pkg_name, output)?;
                }
                Ok(())
            }
            Self::StaleAllowedDuplicates(stale_entries) => {
                write!(f, "Stale entries in `allowed_duplicates` found")?;
                for (pkg_name, entries) in stale_entries {
                    for entry in entries {
                        write!(f, "\n  {}: {}", pkg_name, entry)?;
                    }
                }
                Ok(())
            }
            Self::DeprecatedClippyMsrv(files) => {
                write!(
                    f,
                    "Found MSRV in clippy.toml, use Cargo.toml package.rust-version instead"
                )?;
                for file in files {
                    write!(f, "\n  {}", file)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LintError {}

/// Lint-specific configuration, read from `[package.metadata.rbmt.lint]` in `Cargo.toml`.
#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
struct LintConfig {
    /// List of crate names that are allowed to have duplicate versions.
    allowed_duplicates: Vec<String>,
}

impl LintConfig {
    /// Load lint configuration from `[package.metadata.rbmt.lint]` in the package's `Cargo.toml`.
    fn load(package_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize, Default)]
        struct RbmtTable {
            #[serde(default)]
            lint: LintConfig,
        }

        let path = package_dir.join("Cargo.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str::<PackageManifest<RbmtTable>>(&contents)?.package.metadata.rbmt.lint)
    }
}

/// Run the lint task.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;
    rbmt_eprintln!("Running lint task...");

    lint_workspace(sh)?;
    lint_packages(sh, &packages)?;
    check_duplicate_deps(sh, &packages)?;
    check_cross_package_duplicate_deps(sh)?;
    check_clippy_toml_msrv(sh, &packages)?;

    rbmt_eprintln!("Lint task completed successfully");
    Ok(())
}

/// Lint the workspace with clippy.
fn lint_workspace(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Linting workspace...");

    // Run clippy on workspace with all features.
    cargo_cmd(sh)
        .arg("clippy")
        .arg("--workspace")
        .arg("--all-targets")
        .arg("--all-features")
        .arg("--keep-going")
        .args(&["--", "-D", "warnings"])
        .run_with_capture()?;

    // Run clippy on workspace without features.
    cargo_cmd(sh)
        .arg("clippy")
        .arg("--workspace")
        .arg("--all-targets")
        .arg("--keep-going")
        .args(&["--", "-D", "warnings"])
        .run_with_capture()?;

    Ok(())
}

/// Run extra package-specific lints.
///
/// # Why run at the package level?
///
/// When running `cargo clippy --workspace --no-default-features`, cargo resolves
/// features across the entire workspace, which can enable features through dependencies
/// even when a package's own default features are disabled. Running clippy on each package
/// individually ensures that each package truly compiles and passes lints with only its
/// explicitly enabled features.
fn lint_packages(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Running package-specific lints...");

    let package_names: Vec<_> = packages.iter().map(|p| p.name.as_str()).collect();
    rbmt_eprintln!("Found crates: {}", package_names.join(", "));

    for package in packages {
        // Returns a RAII guard which reverts the working directory to the old value when dropped.
        let _old_dir = sh.push_dir(&package.dir);

        // Run clippy without default features.
        cargo_cmd(sh)
            .arg("clippy")
            .arg("--all-targets")
            .arg("--no-default-features")
            .arg("--keep-going")
            .args(&["--", "-D", "warnings"])
            .run_with_capture()?;
    }

    Ok(())
}

/// Check for duplicate dependencies.
///
/// The goal is to catch cases where a package's transitive dependency tree contains two versions
/// of the same crate (e.g. `bitcoin_hashes v0.13.0` and `bitcoin_hashes v0.14.0` both present). This
/// can happen when a package directly depends on a crate at one version while a transitive
/// dependency pulls in a different version. Downstream users inheriting this package will end up
/// with both versions in their build, which can cause confusing type incompatibility errors across
/// crate boundaries and unnecessarily bloat compile times and binary size.
///
/// Dev dependencies are excluded from this check because they are not part of the published
/// crate graph and cannot cause problems for downstream consumers.
///
/// # Why run at the package level?
///
/// Running per-package allows each package to maintain its own whitelist of allowed duplicates
/// via `rbmt.toml`, since some duplicates may be unavoidable for a given package but not others.
fn check_duplicate_deps(
    sh: &Shell,
    packages: &[Package],
) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Checking for duplicate dependencies...");

    let mut duplicate_deps: Vec<(String, String)> = Vec::new();
    let mut stale_entries: Vec<(String, Vec<String>)> = Vec::new();

    for package in packages {
        let config = LintConfig::load(&package.dir)?;

        // Returns a RAII guard which reverts the working directory to the old value when dropped.
        let _old_dir = sh.push_dir(&package.dir);

        // Run cargo tree to find duplicates for this package, exclude dev dependencies
        // since they are not exposed to downstream consumers.
        let output = cargo_cmd(sh).args(CARGO_TREE_ARGS).ignore_status().read()?;

        let tree = DuplicateTree::parse(&output, &config.allowed_duplicates);

        if !tree.semver_trick_packages().is_empty() {
            rbmt_eprintln!(
                "Found semver trick duplicates in {}: {}",
                package.name,
                tree.semver_trick_packages().join(", ")
            );
        }

        if !tree.duplicates().is_empty() {
            duplicate_deps.push((package.name.clone(), output));
        }
        if !tree.stale_allowed_duplicates().is_empty() {
            stale_entries.push((package.name.clone(), tree.stale_allowed_duplicates().to_vec()));
        }
    }

    if !duplicate_deps.is_empty() {
        return Err(Box::new(LintError::DuplicateDependencies(duplicate_deps)));
    }
    if !stale_entries.is_empty() {
        return Err(Box::new(LintError::StaleAllowedDuplicates(stale_entries)));
    }

    rbmt_eprintln!("No duplicate dependencies found");
    Ok(())
}

/// Check for duplicate dependencies that span multiple workspace members.
///
/// This is a supplementary check to [`check_duplicate_deps`]. Attemps to catch the case where two
/// workspace members depend on different versions of the same crate. For example, if pkg1
/// depends on `bitcoin_hashes 0.13` and pkg2 depends on `bitcoin_hashes 0.14`, each package's
/// individual tree is clean but a downstream consumer of both packages will end up with both
/// versions in their build. Skipped entirely for single-package workspaces since it cannot find
/// anything the per-package check didn't already catch.
///
/// Dev dependencies are excluded from this check because they are not part of the published
/// crate graph and cannot cause problems for downstream consumers.
///
/// This check is not considered as essential as [`check_duplicate_deps`]. A duplicate dependency
/// in a single package has a much higher chance of causing downstream issues than one across
/// packages since it may not be an issue depending on what versions of the packages downstream
/// users are using. This functionality could probably also be accomplished just with
/// [`check_duplicate_deps`] if a workspace contains a facade package which re-exports all
/// the other packages of the workspace.
fn check_cross_package_duplicate_deps(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let package_info = get_workspace_packages(sh, &[])?;

    // No point running a workspace-level check for a single package.
    if package_info.len() <= 1 {
        return Ok(());
    }

    rbmt_eprintln!("Checking for cross-package duplicate dependencies...");

    // Run on all workspace members with the `--workspace` flag.
    let output = cargo_cmd(sh).args(CARGO_TREE_ARGS).arg("--workspace").ignore_status().read()?;

    let tree = DuplicateTree::parse(&output, &[]);
    let cross_package_dupes = tree.cross_package_duplicates();
    // Currently logging a warning instead of hard failure until we gain confidence in the check.
    if !cross_package_dupes.is_empty() {
        rbmt_eprintln!("Found {} cross-package duplicate dependencies", cross_package_dupes.len());
        for (crate_name, versions) in &cross_package_dupes {
            for (version, members) in *versions {
                let members: Vec<&str> = members.iter().map(String::as_str).collect();
                rbmt_eprintln!("  {} {}: {}", crate_name, version, members.join(", "));
            }
        }
    }

    rbmt_eprintln!("No cross-package duplicate dependencies found");
    Ok(())
}

/// A dependency from `cargo tree --duplicates --prefix depth` output.
struct Dependency {
    /// Depth-0 lines are the duplicate crates themselves; all lines beneath them (at any
    /// depth) trace the paths by which that version is included.
    depth: u32,
    /// Name of the crate.
    name: String,
    /// Version of the crate.
    version: String,
    /// Whether this crate is a workspace member.
    is_workspace_member: bool,
}

impl Dependency {
    /// Lines have the form `<depth><name> <version>[ ...]`.
    ///
    /// ```text
    /// 0bitcoin_hashes v0.13.0
    /// 3bip324 v0.10.0 (/home/user/bip324/protocol)
    /// 1bitcoin_hashes v0.16.0 (https://github.com/rust-bitcoin/rust-bitcoin?rev=abc#abc) (*)
    /// ```
    ///
    /// ## Returns
    ///
    /// `None` for lines that don't start with a depth integer like blank lines
    /// or section headers (e.g. `[dev-dependencies]`).
    fn parse(line: &str) -> Option<Self> {
        let depth_digits = line.chars().take_while(char::is_ascii_digit).count();
        let depth: u32 = line[..depth_digits].parse().ok()?;
        let rest = &line[depth_digits..];

        let mut tokens = rest.split_whitespace();
        let name = tokens.next()?.to_string();
        let version = tokens.next()?.to_string();

        // Workspace members have paths like (/path/to/crate).
        // External crates have URLs like (https://...) or special markers like (proc-macro).
        let is_workspace_member = tokens.any(|t| t.starts_with("(/"));

        Some(Self { depth, name, version, is_workspace_member })
    }
}

/// Maps each duplicate crate name to the list of versions found, where each version records which
/// workspace members are responsible for pulling it in (at any depth in the inverted tree).
struct DuplicateTree {
    /// The name of the crate with duplicates mapped to the versions, which are in turn mapped
    /// to the workspace packages which pulled them in. Trees are used to keep keys sorted.
    ///
    /// ```text
    /// "hex-conservative" -> {
    ///     "v0.2.0" -> {"workspace_pkg1"},
    ///     "v0.3.0" -> {"workspace_pkg2", "workspace_pkg3"},
    /// }
    /// ```
    duplicate_crates: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    /// Entries from `allowed_duplicates` that did not appear as actual duplicates
    /// in the tree. These are stale and should be removed from the allowlist.
    stale_allowed: Vec<String>,
    /// Workspace packages detected as using the semver trick pattern. These are intentional
    /// duplicates where the current version depends on a newer version of the same crate.
    semver_tricks: Vec<String>,
}

impl DuplicateTree {
    /// Parse the raw output of `cargo tree --duplicates --prefix depth`.
    fn parse(output: &str, allowed_duplicates: &[String]) -> Self {
        let mut duplicate_crates: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> =
            BTreeMap::new();
        // Current duplicate version being parsed.
        let mut current_duplicate: Option<(String, String)> = None;
        // Track which allowed entries actually appeared as duplicates in the tree.
        let mut seen_allowed_duplicate: HashSet<String> = HashSet::new();
        // Track crates detected as using the semver trick pattern.
        let mut semver_tricks: Vec<String> = Vec::new();

        for line in output.lines() {
            let Some(dep) = Dependency::parse(line) else { continue };

            // Depth-0 is a duplicate crate, we initialize it in the duplicate map, but still
            // need to track down what package is pulling in the duplicate version.
            if dep.depth == 0 {
                // Skip crates that are explicitly allowed to have duplicate versions,
                // but record that they were actually seen as duplicates.
                if allowed_duplicates.iter().any(|a| a == &dep.name) {
                    seen_allowed_duplicate.insert(dep.name.clone());
                    current_duplicate = None;
                    continue;
                }
                // Start of a new version block. Ensure a slot exists for this (name, version).
                duplicate_crates
                    .entry(dep.name.clone())
                    .or_default()
                    .entry(dep.version.clone())
                    .or_default();
                current_duplicate = Some((dep.name, dep.version));
            } else if let Some((ref duplicate_name, ref duplicate_version)) = current_duplicate {
                // Check if this depth-1 line is the same crate at a different version.
                // This is the semver trick pattern, an older version depends on a newer version
                // of the same crate and re-exports its types.
                if dep.depth == 1
                    && dep.name == *duplicate_name
                    && !semver_tricks.contains(duplicate_name)
                {
                    semver_tricks.push(duplicate_name.clone());
                }

                // Any line beneath depth-0 traces the path by which this version is included.
                // Only track workspace members which pull in duplicates (those with actual paths,
                // not external crates).
                if dep.is_workspace_member {
                    if let Some(members) = duplicate_crates
                        .get_mut(duplicate_name)
                        .and_then(|versions| versions.get_mut(duplicate_version))
                    {
                        members.insert(dep.name.clone());
                    }
                }
            }
        }

        // Filter out semver trick duplicates from the results.
        for crate_name in &semver_tricks {
            duplicate_crates.remove(crate_name);
        }

        // Any allowed entry never seen at depth-0 is no longer duplicated and should be removed.
        let stale_allowed = allowed_duplicates
            .iter()
            .filter(|a| !seen_allowed_duplicate.contains(*a))
            .cloned()
            .collect();

        Self { duplicate_crates, stale_allowed, semver_tricks }
    }

    /// All duplicate crates found in the tree.
    fn duplicates(&self) -> &BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
        &self.duplicate_crates
    }

    /// Entries from `allowed_duplicates` that are no longer actually duplicated in the tree.
    fn stale_allowed_duplicates(&self) -> &[String] { &self.stale_allowed }

    /// Duplicates detected as using the semver trick pattern.
    fn semver_trick_packages(&self) -> &[String] { &self.semver_tricks }

    /// Returns cross-package duplicates, crates with different versions pulled in by
    /// different workspace members.
    ///
    /// For example, this is a cross-package duplicate, `pkg1` and `pkg2` each own a different
    /// version, so no per-package fix exists.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1"},
    ///     "v0.2.0" -> {"pkg2"},
    /// }
    /// ```
    ///
    /// This is *not* a cross-package duplicate, `pkg1` appears in both version blocks, so it
    /// alone is responsible and the per-package check will catch it.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1"},
    ///     "v0.2.0" -> {"pkg1"},
    /// }
    /// ```
    ///
    /// This is also not a cross-package duplicate, since both will get caught at the per-package check.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1", "pkg2"},
    ///     "v0.2.0" -> {"pkg1", "pkg2"},
    /// }
    /// ```
    ///
    /// This is a cross-package duplicate since `pkg3` pulls in a whole new version.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1", "pkg2"},
    ///     "v0.2.0" -> {"pkg1", "pkg2"},
    ///     "v0.3.0" -> {"pkg3"},
    /// }
    /// ```
    ///
    /// Here is a doozy though. Is this a cross package duplicate? It is reported as *no*, not a duplicate.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1", "pkg2", "pkg3"},
    ///     "v0.2.0" -> {"pkg1"},
    /// }
    /// ```
    ///
    /// And this? Also reported as *no* since it is hard to detect and maybe the first step is to deal with `pkg1`
    /// which is caught by the per-package check.
    ///
    /// ```text
    /// "foo" -> {
    ///     "v0.1.0" -> {"pkg1", "pkg2", "pkg3"},
    ///     "v0.2.0" -> {"pkg1", "pkg2"},
    ///     "v0.3.0" -> {"pkg1", "pkg3"}
    /// }
    /// ```
    ///
    ///
    /// ## Returns
    ///
    /// A map from crate name to its versions and the members responsible for each. The map is
    /// empty if no cross-package duplicates were found. For example, given the first example
    /// above the return value would be:
    ///
    /// ```text
    /// { "foo" -> { "v0.1.0" -> {"pkg1"}, "v0.2.0" -> {"pkg2"} } }
    /// ```
    fn cross_package_duplicates(&self) -> BTreeMap<&str, &BTreeMap<String, BTreeSet<String>>> {
        self.duplicate_crates
            .iter()
            // Filter out per-package duplicates.
            .filter(|(_, versions)| {
                // Cross-package if no single member appears in every version block.
                !versions
                    .values()
                    .flat_map(|members| members.iter())
                    .any(|m| versions.values().all(|s| s.contains(m)))
            })
            .map(|(crate_name, versions)| (crate_name.as_str(), versions))
            .collect()
    }
}

/// Check for deprecated clippy.toml MSRV settings.
///
/// The bitcoin ecosystem has moved to Rust 1.74+ and should use Cargo.toml
/// package.rust-version instead of clippy.toml msrv settings.
fn check_clippy_toml_msrv(
    sh: &Shell,
    packages: &[Package],
) -> Result<(), Box<dyn std::error::Error>> {
    const CLIPPY_CONFIG_FILES: &[&str] = &["clippy.toml", ".clippy.toml"];

    rbmt_eprintln!("Checking for deprecated clippy.toml MSRV settings...");

    let mut clippy_files = Vec::new();

    // Check workspace root.
    let workspace_root = get_workspace_root(sh)?;
    for filename in CLIPPY_CONFIG_FILES {
        let path = workspace_root.join(filename);
        if path.exists() {
            clippy_files.push(path);
        }
    }

    // Check each package.
    for package in packages {
        for filename in CLIPPY_CONFIG_FILES {
            let path = package.dir.join(filename);
            if path.exists() {
                clippy_files.push(path);
            }
        }
    }

    // Check each clippy file for the msrv setting.
    let mut problematic_files = Vec::new();
    for path in clippy_files {
        let contents = fs::read_to_string(&path)?;
        let config: toml::Value = toml::from_str(&contents)?;

        if config.get("msrv").is_some() {
            problematic_files.push(path.display().to_string());
        }
    }

    if !problematic_files.is_empty() {
        return Err(Box::new(LintError::DeprecatedClippyMsrv(problematic_files)));
    }

    rbmt_eprintln!("No deprecated clippy.toml MSRV settings found");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_package_duplicate() {
        // pkg1 and pkg2 each pull in different versions of bitcoin_hashes directly.
        // hex-conservative is pulled in transitively via bitcoin_hashes, but pkg1
        // and pkg2 appear beneath each hex-conservative version block too, so
        // it is also reported as a cross-package duplicate.
        let output = "\
0bitcoin_hashes v0.13.0
1pkg1 v0.1.0 (/path/to/pkg1)

0bitcoin_hashes v0.14.1
1pkg2 v0.1.0 (/path/to/pkg2)

0hex-conservative v0.1.2
1bitcoin_hashes v0.13.0 (*)
2pkg1 v0.1.0 (/path/to/pkg1)

0hex-conservative v0.2.2
1bitcoin_hashes v0.14.1 (*)
2pkg2 v0.1.0 (/path/to/pkg2)
";
        let tree = DuplicateTree::parse(output, &[]);
        let dupes = tree.cross_package_duplicates();
        assert!(dupes.contains_key("bitcoin_hashes"));
        assert!(dupes.contains_key("hex-conservative"));
        assert!(dupes["bitcoin_hashes"].contains_key("v0.13.0"));
        assert!(dupes["bitcoin_hashes"].contains_key("v0.14.1"));
        assert!(dupes["hex-conservative"].contains_key("v0.1.2"));
        assert!(dupes["hex-conservative"].contains_key("v0.2.2"));
    }

    #[test]
    fn cross_package_transitive_duplicates() {
        let output = "\
0hex-conservative v0.1.2
1some-lib v1.0.0
2pkg1 v0.1.0

0hex-conservative v0.2.2
1some-lib v2.0.0
2pkg2 v0.1.0
";
        let tree = DuplicateTree::parse(output, &[]);
        let dupes = tree.cross_package_duplicates();
        assert!(dupes.contains_key("hex-conservative"));
        assert!(dupes["hex-conservative"].contains_key("v0.1.2"));
        assert!(dupes["hex-conservative"].contains_key("v0.2.2"));
    }

    #[test]
    fn cross_package_single_package_not_reported() {
        let output = "\
0foo v0.1.0
1pkg1 v0.1.0 (/path/to/pkg1)

0foo v0.2.0
1pkg1 v0.1.0 (/path/to/pkg1)
";
        let tree = DuplicateTree::parse(output, &[]);
        assert!(tree.cross_package_duplicates().is_empty());
    }

    #[test]
    fn cross_package_dedupe_output() {
        let output = "\
0bitcoin_hashes v0.13.0
1pkg1 v0.1.0 (/path/to/pkg1)

0bitcoin_hashes v0.14.1
1pkg2 v0.1.0 (/path/to/pkg2)

0bitcoin_hashes v0.13.0
1pkg1 v0.1.0 (/path/to/pkg1)

0bitcoin_hashes v0.14.1
1pkg2 v0.1.0 (/path/to/pkg2)
";
        let tree = DuplicateTree::parse(output, &[]);
        let dupes = tree.cross_package_duplicates();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes["bitcoin_hashes"]["v0.13.0"], BTreeSet::from(["pkg1".to_string()]));
        assert_eq!(dupes["bitcoin_hashes"]["v0.14.1"], BTreeSet::from(["pkg2".to_string()]));
    }

    #[test]
    fn cross_package_shared_packages_across_all_dupes() {
        let output = "\
0foo v0.1.0
1pkg1 v0.1.0 (/path/to/pkg1)
1pkg2 v0.1.0 (/path/to/pkg2)

0foo v0.2.0
1pkg1 v0.1.0 (/path/to/pkg1)
1pkg2 v0.1.0 (/path/to/pkg2)
";
        let tree = DuplicateTree::parse(output, &[]);
        assert!(tree.cross_package_duplicates().is_empty());
    }

    #[test]
    fn cross_package_empty_output_no_dupes() {
        let tree = DuplicateTree::parse("", &[]);
        assert!(tree.cross_package_duplicates().is_empty());
    }

    #[test]
    fn allowed_duplicates_not_reported() {
        let output = "\
0bitcoin_hashes v0.13.0
1pkg1 v0.1.0

0bitcoin_hashes v0.14.1
1pkg2 v0.1.0

0hex-conservative v0.1.2
1pkg1 v0.1.0

0hex-conservative v0.2.2
1pkg2 v0.1.0
";
        let allowed = vec!["bitcoin_hashes".to_string()];
        let tree = DuplicateTree::parse(output, &allowed);
        let dupes = tree.duplicates();
        assert!(!dupes.contains_key("bitcoin_hashes"), "allowed duplicate should be filtered");
        assert!(dupes.contains_key("hex-conservative"), "non-allowed duplicate should be reported");
    }

    #[test]
    fn stale_allowed_duplicates_reported() {
        let output = "\
0hex-conservative v0.1.2
1pkg1 v0.1.0

0hex-conservative v0.2.2
1pkg2 v0.1.0
";
        // bitcoin_hashes is in the allowlist but not present in the tree at all.
        let allowed = vec!["bitcoin_hashes".to_string(), "hex-conservative".to_string()];
        let tree = DuplicateTree::parse(output, &allowed);
        let stale = tree.stale_allowed_duplicates();
        assert_eq!(stale, &["bitcoin_hashes".to_string()]);
        assert!(!stale.contains(&"hex-conservative".to_string()));
    }

    #[test]
    fn no_stale_allowed_duplicates_when_all_present() {
        let output = "\
0bitcoin_hashes v0.13.0
1pkg1 v0.1.0

0bitcoin_hashes v0.14.1
1pkg2 v0.1.0
";
        let allowed = vec!["bitcoin_hashes".to_string()];
        let tree = DuplicateTree::parse(output, &allowed);
        assert!(tree.stale_allowed_duplicates().is_empty());
    }

    #[test]
    fn empty_allowlist_has_no_stale_entries() {
        let output = "\
0foo v0.1.0
1pkg1 v0.1.0

0foo v0.2.0
1pkg2 v0.1.0
";
        let tree = DuplicateTree::parse(output, &[]);
        assert!(tree.stale_allowed_duplicates().is_empty());
    }

    #[test]
    fn semver_trick_detected_and_filtered() {
        let output = "\
0units v0.4.1
1units v0.5.0 (https://github.com/...)
2other_dep v1.0.0

0units v0.5.0
1another_dep v2.0.0
";
        let tree = DuplicateTree::parse(output, &[]);
        // Both versions should be filtered out as semver trick.
        assert!(tree.duplicates().is_empty(), "semver trick duplicates should be filtered");
        assert!(tree.semver_trick_packages().contains(&"units".to_string()));
    }

    #[test]
    fn semver_trick_only_filters_matching_crate() {
        let output = "\
0my_crate v0.4.1
1my_crate v0.5.0 (https://...)

0my_crate v0.5.0
1other_dep v1.0.0

0hex_conservative v0.1.0
1pkg1 v0.1.0

0hex_conservative v0.2.0
1pkg2 v0.1.0
";
        let tree = DuplicateTree::parse(output, &[]);
        // my_crate should be filtered as semver trick.
        assert!(!tree.duplicates().contains_key("my_crate"));
        assert!(tree.duplicates().contains_key("hex_conservative"));
        assert!(tree.semver_trick_packages().contains(&"my_crate".to_string()));
    }
}
