//! Build and test tasks with feature matrix testing.
//!
//! `cargo build` runs before `cargo test` throughout this module to try
//! and catch any issues involving `cfg(test)` somehow gating required code.

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::Deserialize;
use xshell::{Cmd, Shell};

use crate::environment::{
    discover_features, git_commit_id, quiet_println, Package, PackageManifest,
};
use crate::toolchain::{prepare_toolchain, Toolchain};
use crate::{git, quiet_cmd};

/// Summary of everything tested for a single package.
#[derive(Debug, Default)]
struct PackageSummary {
    /// Manifest name of the package.
    name: String,
    /// Examples that were run, in their as-configured form (e.g. `"bip32:-"`).
    examples: Vec<String>,
    /// Individual auto-discovered features tested in isolation.
    individual_features: Vec<String>,
    /// Random commit-seeded feature subsets that were tested.
    sampled_subsets: Vec<Vec<String>>,
    /// Exact feature sets from `exact_features` config that were tested.
    exact_sets: Vec<Vec<String>>,
    /// Whether the no-std cross-compilation check was run.
    no_std_checked: bool,
}

impl fmt::Display for PackageSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Pretty print a list of features.
        let fmt_list = |list: &[String]| -> String {
            if list.is_empty() {
                "(none)".to_string()
            } else {
                list.join(", ")
            }
        };
        // Pretty print a list of feature sets.
        let fmt_sets = |sets: &[Vec<String>]| -> String {
            if sets.is_empty() {
                return "(none)".to_string();
            }
            sets.iter().map(|s| format!("[{}]", fmt_list(s))).collect::<Vec<_>>().join(", ")
        };

        let rows: &[(&str, String)] = &[
            ("Examples", fmt_list(&self.examples)),
            ("Individual features", fmt_list(&self.individual_features)),
            ("Sampled subsets", fmt_sets(&self.sampled_subsets)),
            ("Exact sets", fmt_sets(&self.exact_sets)),
            ("No-std check", if self.no_std_checked { "ran" } else { "skipped" }.to_string()),
        ];

        // Compute the column width from the longest label so values align.
        let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
        writeln!(f, "  Package: {}", self.name)?;
        for (label, value) in rows {
            writeln!(f, "    {label:<width$}: {value}")?;
        }

        Ok(())
    }
}

/// Summary of an entire test run, grouped by commit.
#[derive(Debug, Default)]
struct TestSummary {
    // Commit SHA paired with the package summaries tested at that commit.
    commits: Vec<(String, Vec<PackageSummary>)>,
}

impl TestSummary {
    fn print(&self) {
        quiet_println("Test Summary");
        for (sha, packages) in &self.commits {
            quiet_println(&format!("Commit: {}", sha));
            for pkg in packages {
                quiet_println(&pkg.to_string());
            }
        }
    }
}

/// Test-specific configuration, read from `[package.metadata.rbmt.test]` in `Cargo.toml`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TestConfig {
    /// Examples to run with different feature configurations.
    ///
    /// Supported formats:
    /// * `"name"` - runs with default features.
    /// * `"name:-"` - runs with no-default-features.
    /// * `"name:feature1 feature2"` - runs with specific features.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [package.metadata.rbmt.test]
    /// examples = [
    ///     "bip32",
    ///     "bip32:-",
    ///     "bip32:serde rand"
    /// ]
    /// ```
    examples: Vec<String>,

    /// Features to exclude from automatic feature discovery.
    ///
    /// Use this list to skip features that should not be tested in isolation,
    /// such as internal or alias features.
    exclude_features: Vec<String>,

    /// Exact feature combinations to always test.
    exact_features: Vec<Vec<String>>,

    /// Always run tests with `--release` for this package.
    release: bool,
}

impl TestConfig {
    /// Load test configuration from `[package.metadata.rbmt.test]` in the package's `Cargo.toml`.
    fn load(crate_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize, Default)]
        struct RbmtTable {
            #[serde(default)]
            test: TestConfig,
        }

        let path = crate_dir.join("Cargo.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str::<PackageManifest<RbmtTable>>(&contents)?.package.metadata.rbmt.test)
    }
}

/// Build and test with the given features and optional `--release` flag.
fn test_features(
    sh: &Shell,
    features: &[impl AsRef<str>],
    release: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let features_str = features.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" ");
    quiet_println(&format!("Testing features: {}", features_str));
    with_release(
        quiet_cmd!(sh, "cargo --locked build --no-default-features --features={features_str}"),
        release,
    )
    .run()?;
    with_release(
        quiet_cmd!(sh, "cargo --locked test --no-default-features --features={features_str}"),
        release,
    )
    .run()?;
    Ok(())
}

/// Conditionally append `--release` to a cargo command.
fn with_release(cmd: Cmd<'_>, release: bool) -> Cmd<'_> {
    if release {
        cmd.arg("--release")
    } else {
        cmd
    }
}

/// Run build and test for all crates with the specified toolchain.
///
/// If `baseline` is `Some`, checks out each commit between `baseline` and HEAD in turn,
/// running the full test suite at each one. The checkout is restored via
/// [`git::GitSwitchGuard`] even on failure, and the run stops immediately if any commit fails.
pub fn run(
    sh: &Shell,
    toolchain: Toolchain,
    no_debug_assertions: bool,
    release: bool,
    baseline: Option<&str>,
    packages: &[Package],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut summary = TestSummary::default();

    if let Some(baseline) = baseline {
        let commits = git::list_commits(sh, baseline)?;
        if commits.is_empty() {
            quiet_println(&format!("No commits found between '{}' and HEAD.", baseline));
            return Ok(());
        }
        quiet_println(&format!("Testing {} commit(s) against baseline '{}'", commits.len(), baseline));
        for sha in &commits {
            quiet_println(&format!("Testing commit {}...", &sha[..12]));
            let _guard = git::GitSwitchGuard::new(sh, sha)?;
            let pkg_summaries = test_commit(sh, toolchain, no_debug_assertions, release, packages)?;
            summary.commits.push((sha.clone(), pkg_summaries));
        }
    } else {
        let sha = git_commit_id(sh).unwrap_or_else(|| "unknown".to_owned());
        let pkg_summaries = test_commit(sh, toolchain, no_debug_assertions, release, packages)?;
        summary.commits.push((sha, pkg_summaries));
    }

    summary.print();
    Ok(())
}

/// Run the full test suite at the current commit and return the per-package summaries.
fn test_commit(
    sh: &Shell,
    toolchain: Toolchain,
    no_debug_assertions: bool,
    release: bool,
    packages: &[Package],
) -> Result<Vec<PackageSummary>, Box<dyn std::error::Error>> {
    quiet_println(&format!("Testing {} crate(s)", packages.len()));

    // Configure RUSTFLAGS for debug assertions.
    let _env = sh.push_env(
        "RUSTFLAGS",
        if no_debug_assertions { "-C debug-assertions=off" } else { "-C debug-assertions=on" },
    );

    let mut pkg_summaries = Vec::new();

    for package in packages {
        let (package_name, package_dir) = package;
        quiet_println(&format!("Testing package: {}", package_name));

        let _dir = sh.push_dir(package_dir);
        // prepare_toolchain is called per-package because MSRV is read from
        // each package's Cargo.toml individually rather than the workspace root.
        prepare_toolchain(sh, toolchain)?;
        let config = TestConfig::load(Path::new(package_dir))?;
        let release = release || config.release;

        let mut pkg_summary = PackageSummary { name: package_name.clone(), ..Default::default() };

        do_test(sh, &config, release, &mut pkg_summary)?;
        do_feature_matrix(sh, package, &config, release, &mut pkg_summary)?;
        do_no_std_check(sh, Path::new(package_dir), &mut pkg_summary)?;

        pkg_summaries.push(pkg_summary);
    }

    Ok(pkg_summaries)
}

/// Run basic build, test, and examples.
fn do_test(
    sh: &Shell,
    config: &TestConfig,
    release: bool,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running basic tests");

    // Basic build and test.
    with_release(quiet_cmd!(sh, "cargo --locked build"), release).run()?;
    with_release(quiet_cmd!(sh, "cargo --locked test"), release).run()?;

    // Run examples.
    for example in &config.examples {
        let parts: Vec<&str> = example.split(':').collect();

        match parts.len() {
            1 => {
                // Format: "name" - run with default features.
                let name = parts[0];
                with_release(quiet_cmd!(sh, "cargo --locked run --example {name}"), release)
                    .run()?;
            }
            2 => {
                let name = parts[0];
                let features = parts[1];

                if features == "-" {
                    // Format: "name:-" - run with no-default-features.
                    with_release(
                        quiet_cmd!(sh, "cargo --locked run --no-default-features --example {name}"),
                        release,
                    )
                    .run()?;
                } else {
                    // Format: "name:features" - run with specific features.
                    with_release(
                        quiet_cmd!(sh, "cargo --locked run --example {name} --features={features}"),
                        release,
                    )
                    .run()?;
                }
            }
            _ => {
                return Err(format!(
                    "Invalid example format: {}, expected 'name', 'name:-', or 'name:features'",
                    example
                )
                .into());
            }
        }

        summary.examples.push(example.clone());
    }

    Ok(())
}

/// Run feature matrix tests.
///
/// 1. All features (unconditional)
/// 2. No features (unconditional)
/// 3. Auto-discovered features individually + sampled subsets per commit (unconditional)
/// 4. Exact feature sets (when configured)
fn do_feature_matrix(
    sh: &Shell,
    package: &Package,
    config: &TestConfig,
    release: bool,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running feature matrix tests");

    // Test all features.
    quiet_println("Testing all features");
    with_release(quiet_cmd!(sh, "cargo --locked build --all-features"), release).run()?;
    with_release(quiet_cmd!(sh, "cargo --locked test --all-features"), release).run()?;

    // Test no features.
    quiet_println("Testing no features");
    with_release(quiet_cmd!(sh, "cargo --locked build --no-default-features"), release).run()?;
    with_release(quiet_cmd!(sh, "cargo --locked test --no-default-features"), release).run()?;

    // Test each feature in isolation, plus sampled subsets.
    let features: Vec<String> = discover_features(sh, package)?
        .into_iter()
        .filter(|f| !config.exclude_features.contains(f))
        .collect();
    if !features.is_empty() {
        quiet_println(&format!(
            "Discovered {} feature(s) to test: {}",
            features.len(),
            features.join(", ")
        ));
        sampled_feature_matrix(sh, &features, release, summary)?;
    }

    // Test exact feature sets.
    for features in &config.exact_features {
        test_features(sh, features, release)?;
        summary.exact_sets.push(features.clone());
    }

    Ok(())
}

/// Test auto-discovered features with per-commit random sampling.
///
/// Runs each feature individually (always), plus `ceil(log2(n))` random feature subsets
/// where `n` is the number of features. The subsets are selected based on the commit ID,
/// so are deterministic for a given commit.
///
/// *Warning!* When no commit ID is available (not in a git repo), only the individual
/// feature runs are performed.
fn sampled_feature_matrix(
    sh: &Shell,
    features: &[String],
    release: bool,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    // ceil(log2(n)) scales the number of random subsets with the feature count.
    fn num_subsets(n: usize) -> u32 {
        if n <= 1 {
            0
        } else {
            n.ilog2() + u32::from(!n.is_power_of_two())
        }
    }

    // Test each feature individually.
    for feature in features {
        test_features(sh, &[feature], release)?;
        summary.individual_features.push(feature.clone());
    }

    // Test random feature subsets, scaling with feature count.
    if let Some(commit) = git_commit_id(sh) {
        for subset_index in 0..num_subsets(features.len()) {
            let subset: Vec<&String> = features
                .iter()
                // Uses the low bit of a hash the [seed + feature name] to determine membership.
                .filter(|f| {
                    let mut hasher = DefaultHasher::new();
                    commit.hash(&mut hasher);
                    subset_index.hash(&mut hasher);
                    f.hash(&mut hasher);
                    hasher.finish() & 1 == 1
                })
                .collect();

            if subset.is_empty() {
                continue;
            }

            test_features(sh, &subset, release)?;
            summary.sampled_subsets.push(subset.into_iter().cloned().collect());
        }
    }

    Ok(())
}

/// Detect if a package is attempting to be no-std.
fn is_no_std_package(sh: &Shell, package_dir: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    // Use cargo metadata to find the library target's source path.
    let metadata = quiet_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    // Find the package matching our directory.
    let packages =
        json["packages"].as_array().ok_or("Missing 'packages' field in cargo metadata")?;
    let current_manifest = package_dir.join("Cargo.toml");
    let package = packages
        .iter()
        .find(|p| {
            p["manifest_path"].as_str().is_some_and(|path| Path::new(path) == current_manifest)
        })
        .ok_or("Could not find package in metadata")?;

    // Find the lib source file.
    let targets = package["targets"].as_array().ok_or("Missing 'targets' field")?;
    let lib_target = targets
        .iter()
        .find(|t| t["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|k| k == "lib")));
    let Some(lib_target) = lib_target else {
        return Ok(false);
    };
    let lib_path = lib_target["src_path"].as_str().ok_or("Missing src_path in lib target")?;

    // Check for #![no_std] attribute.
    let contents = std::fs::read_to_string(lib_path)?;
    Ok(contents.lines().any(|line| line.trim() == "#![no_std]"))
}

/// Check no-std compatibility if the package declares `#![no_std]`.
fn do_no_std_check(
    sh: &Shell,
    package_dir: &Path,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    const NO_STD_TARGET: &str = "thumbv7m-none-eabi";
    if !is_no_std_package(sh, package_dir)? {
        return Ok(());
    }

    quiet_println(&format!("Detected no-std package, building for target: {}", NO_STD_TARGET));
    quiet_cmd!(sh, "cargo build --target {NO_STD_TARGET} --no-default-features").run()?;
    quiet_println("no-std build passed!");
    summary.no_std_checked = true;
    Ok(())
}
