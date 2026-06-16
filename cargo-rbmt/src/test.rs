// SPDX-License-Identifier: MIT AND Apache-2.0

//! Build and test tasks with feature matrix testing.
//!
//! `cargo build` runs before `cargo test` throughout this module to try
//! and catch any issues involving `cfg(test)` somehow gating required code.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{
    cargo_cmd, discover_features, get_workspace_packages, git_commit_id, CmdExt, Package,
    PackageManifest, ProgressGuard,
};
use crate::git;
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain_with_override, Toolchain};

/// Feature to MSRV version mappings for override during testing.
#[derive(Debug, Clone, Default, Deserialize)]
struct MsrvOverrides(std::collections::HashMap<String, String>);

impl MsrvOverrides {
    /// Find an MSRV override for the given features.
    ///
    /// Returns the override version if any feature in the set has an override, `None` otherwise.
    /// Returns an error if multiple features have conflicting MSRV overrides.
    ///
    /// # Arguments
    ///
    /// * `features` - `None` checks all configured overrides, `Some(features)` checks overrides
    ///   for those specific features.
    fn get(&self, features: Option<&[String]>) -> Result<Option<&str>, Box<dyn std::error::Error>> {
        let overrides: HashSet<_> = match features {
            None => {
                // Get all configured overrides.
                self.0.values().map(std::string::String::as_str).collect()
            }
            Some(feature_list) => {
                // Get overrides for specific features.
                feature_list
                    .iter()
                    .filter_map(|f| self.0.get(f).map(std::string::String::as_str))
                    .collect()
            }
        };

        match overrides.len() {
            0 => Ok(None),
            1 => Ok(overrides.into_iter().next()),
            _ => Err(format!("Conflicting MSRV overrides: {:?}", overrides).into()),
        }
    }
}

/// Strategy for sampling feature combinations during testing.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SampleStrategy {
    /// Use logarithmic sampling `ceil(log2(n))` random subsets per commit.
    #[default]
    Log,
    /// Test all possible feature combinations (excluding none, individual, and all).
    All,
}

impl SampleStrategy {
    /// Generate feature subsets according to this sampling strategy.
    ///
    /// Returns a vector of feature subsets to test. The exact subsets depend on the strategy.
    ///
    /// * **Log**: Generates `ceil(log2(n))` random subsets based on the current commit ID.
    ///   If no commit ID is available, returns an empty vector.
    /// * **All**: Generates all (~`2^n`) possible combinations. Excludes empty, individual, and
    ///   full set which are tested elsewhere.
    fn generate_subsets(self, features: &[String], commit: Option<String>) -> Vec<Vec<String>> {
        match self {
            Self::Log => generate_log_sampled_subsets(features, commit),
            Self::All => generate_all_subsets(features),
        }
    }
}

/// Generate logarithmic sampled feature subsets.
///
/// Generates `ceil(log2(n))` random feature subsets where `n` is the number of features.
/// The subsets are selected based on the commit ID, so are deterministic for a given commit.
///
/// If no commit ID is available, returns an empty vector.
fn generate_log_sampled_subsets(features: &[String], commit: Option<String>) -> Vec<Vec<String>> {
    let Some(commit) = commit else {
        return Vec::new();
    };

    let n = features.len() as u32;
    let max_index = if n == 0 { 0 } else { n.ilog2() + u32::from(!n.is_power_of_two()) };

    let mut subsets = Vec::new();

    for index in 0..max_index {
        let subset: Vec<String> = features
            .iter()
            .filter(|f| {
                let mut hasher = DefaultHasher::new();
                commit.hash(&mut hasher);
                index.hash(&mut hasher);
                f.hash(&mut hasher);
                hasher.finish() & 1 == 1
            })
            .cloned()
            .collect();

        if !subset.is_empty() {
            subsets.push(subset);
        }
    }

    subsets
}

/// Generate all possible feature subsets.
///
/// Generates all possible combinations of 2 or more features (excluding empty, individual,
/// and full set). Individual feature combinations are tested separately by
/// `sampled_feature_matrix`.
fn generate_all_subsets(features: &[String]) -> Vec<Vec<String>> {
    if features.len() < 2 {
        return Vec::new();
    }

    let mut subsets = Vec::new();

    // Iterate through all bitmask combinations from 1 (because 0 is empty set) to `2^n - 2`
    // (because `2^n - 1` is all features).
    for mask in 1usize..=(1 << features.len()) - 2 {
        // Skip if only one bit is set (individual feature).
        if mask.is_power_of_two() {
            continue;
        }

        // Collect features corresponding to set bits in the mask.
        let mut subset = Vec::new();
        for (idx, feature) in features.iter().enumerate() {
            if (mask >> idx) & 1 == 1 {
                subset.push(feature.clone());
            }
        }
        subsets.push(subset);
    }

    subsets
}

/// Summary of everything tested for a single package.
#[derive(Debug, Default)]
struct PackageSummary {
    /// Manifest name of the package.
    name: String,
    /// Examples that were run, in their as-configured form (e.g. `"bip32:-"`).
    examples: Vec<String>,
    /// Individual auto-discovered features tested in isolation.
    individual_features: Vec<String>,
    /// Feature subsets which were tested.
    feature_subsets: Vec<Vec<String>>,
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
            ("Feature subsets", fmt_sets(&self.feature_subsets)),
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
    /// Print summary to stdout.
    fn print(&self) {
        println!("Test Summary");
        for (sha, packages) in &self.commits {
            println!("Commit: {}", sha);
            for pkg in packages {
                print!("{}", pkg);
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
    /// * `"name"` - runs with no features.
    /// * `"name:feature1 feature2"` - runs with specific features.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [package.metadata.rbmt.test]
    /// examples = [
    ///     "bip32",
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

    /// Strategy for sampling feature combinations during testing.
    ///
    /// Options:
    /// * `"log"` - Logarithmic sampling (default) `ceil(log2(n))` random subsets per commit.
    /// * `"all"` - Test all combinations: 2^n - 2 subsets (excluding none, individual, and all).
    ///
    /// # Examples
    ///
    /// ```toml
    /// [package.metadata.rbmt.test]
    /// sample_strategy = "all"
    /// ```
    #[serde(default)]
    sample_strategy: SampleStrategy,

    /// Feature-specific MSRV overrides.
    ///
    /// If a feature is enabled during testing, use this MSRV instead of the default.
    /// Useful when certain features require a minimum Rust version higher than the package MSRV.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [package.metadata.rbmt.test]
    /// msrv_overrides = { "some-feature" = "1.75.0", "another-feature" = "1.75.0" }
    /// ```
    #[serde(default)]
    msrv_overrides: MsrvOverrides,
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

/// Build and test with the given features and cargo test arguments.
///
/// If any feature has an MSRV override configured, uses that MSRV instead of the default.
///
/// # Arguments
///
/// * `feature_selection` - `None` means `--all-features`, `Some([])` means `--no-default-features`
///   with no features, `Some(["feat1", ...])` means `--no-default-features --features feat1 ...`
fn test_features(
    sh: &Shell,
    toolchain: Toolchain,
    feature_selection: Option<&[String]>,
    cargo_args: &[String],
    msrv_overrides: &MsrvOverrides,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check for MSRV override.
    let msrv_override = msrv_overrides.get(feature_selection)?;
    prepare_toolchain_with_override(sh, toolchain, msrv_override)?;

    match feature_selection {
        None => {
            // Test all features.
            cargo_cmd(sh).arg("build").arg("--all-features").args(cargo_args).run_with_capture()?;
            cargo_cmd(sh).arg("test").arg("--all-features").args(cargo_args).run_with_capture()?;
        }
        Some(features) => {
            // Test specific features (or no features if empty).
            let mut build_cmd = cargo_cmd(sh).arg("build").arg("--no-default-features");
            if !features.is_empty() {
                // Avoid issues with feature names which contain a hyphen.
                build_cmd = build_cmd.arg("--features").arg(features.join(","));
            }
            build_cmd.args(cargo_args).run_with_capture()?;

            let mut test_cmd = cargo_cmd(sh).arg("test").arg("--no-default-features");
            if !features.is_empty() {
                // Avoid issues with feature names which contain a hyphen.
                test_cmd = test_cmd.arg("--features").arg(features.join(","));
            }
            test_cmd.args(cargo_args).run_with_capture()?;
        }
    }

    Ok(())
}

/// Run build and test for all crates with the specified toolchain.
///
/// If `baseline` is `Some`, checks out each commit between `baseline` and HEAD in turn,
/// running the full test suite at each one. The checkout is restored via
/// [`git::GitSwitchGuard`] even on failure, and the run stops immediately if any commit fails.
///
/// # Arguments
///
/// * `sh` - The shell environment.
/// * `lockfile` - Which lockfile variant to use.
/// * `toolchain` - Which toolchain to use.
/// * `baseline` - Optional baseline ref for testing multiple commits.
/// * `packages` - Packages to test (empty = all).
/// * `cargo_args` - Additional arguments to pass to cargo build and test commands.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    toolchain: Toolchain,
    baseline: Option<&str>,
    packages: &[String],
    cargo_args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut progress = ProgressGuard::new();
    let mut summary = TestSummary::default();

    if let Some(baseline) = baseline {
        let commits = git::list_commits(sh, baseline)?;
        if commits.is_empty() {
            rbmt_eprintln!("No commits found between '{}' and HEAD.", baseline);
            return Ok(());
        }
        rbmt_eprintln!("Testing {} commit(s) against baseline '{}'", commits.len(), baseline);
        for sha in &commits {
            rbmt_eprintln!("Testing commit {}...", &sha[..12]);
            // Switch to the commit first, then use lockfile on that commit in case
            // there are lockfile updates. Guards will unwind in reverse order (LIFO).
            let _git_guard = git::GitSwitchGuard::new(sh, sha)?;
            let _lockfile_guard = lockfile.activate(sh)?;
            // Resolve packages for each commit, so we only test packages that exist in that commit.
            let packages = get_workspace_packages(sh, packages)?;
            let pkg_summaries = test_commit(sh, toolchain, &packages, cargo_args)?;
            summary.commits.push((sha.clone(), pkg_summaries));
        }
    } else {
        let packages = get_workspace_packages(sh, packages)?;
        let _lockfile_guard = lockfile.activate(sh)?;
        let sha = git_commit_id(sh).unwrap_or_else(|| "unknown".to_owned());
        let pkg_summaries = test_commit(sh, toolchain, &packages, cargo_args)?;
        summary.commits.push((sha, pkg_summaries));
    }

    rbmt_eprintln!("Tests complete.");
    progress.disable();
    summary.print();
    Ok(())
}

/// Run the full test suite at the current commit and return the per-package summaries.
fn test_commit(
    sh: &Shell,
    toolchain: Toolchain,
    packages: &[Package],
    cargo_args: &[String],
) -> Result<Vec<PackageSummary>, Box<dyn std::error::Error>> {
    rbmt_eprintln!("Testing {} crate(s)", packages.len());

    let mut pkg_summaries = Vec::new();

    for package in packages {
        rbmt_eprintln!("Testing package: {}", package.name);

        let _dir = sh.push_dir(&package.dir);
        let config = TestConfig::load(Path::new(&package.dir))?;

        let mut pkg_summary = PackageSummary { name: package.name.clone(), ..Default::default() };

        do_examples(sh, toolchain, &config, &mut pkg_summary)?;
        do_feature_matrix(sh, toolchain, package, &config, cargo_args, &mut pkg_summary)?;
        do_no_std_check(sh, &package.dir, &mut pkg_summary)?;

        pkg_summaries.push(pkg_summary);
    }

    Ok(pkg_summaries)
}

/// Run examples.
fn do_examples(
    sh: &Shell,
    toolchain: Toolchain,
    config: &TestConfig,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Running examples in {}", summary.name);

    for example in &config.examples {
        let parts: Vec<&str> = example.split(':').collect();

        match parts.len() {
            1 => {
                let name = parts[0];
                rbmt_eprintln!("Running example {} with no features in {}", name, summary.name);
                prepare_toolchain_with_override(sh, toolchain, None)?;

                cargo_cmd(sh)
                    .arg("run")
                    .arg("--no-default-features")
                    .arg("--example")
                    .arg(name)
                    .run_with_capture()?;
            }
            2 => {
                let name = parts[0];
                let features: Vec<String> =
                    parts[1].split_whitespace().map(std::string::ToString::to_string).collect();

                rbmt_eprintln!(
                    "Running example {} with features {:?} in {}",
                    name,
                    features,
                    summary.name
                );

                // Prepare toolchain with any MSRV override for these features.
                let msrv_override = config.msrv_overrides.get(Some(&features))?;
                prepare_toolchain_with_override(sh, toolchain, msrv_override)?;

                cargo_cmd(sh)
                    .arg("run")
                    .arg("--no-default-features")
                    .arg("--example")
                    .arg(name)
                    .arg("--features")
                    // Avoid issues with feature names which contain a hyphen.
                    .arg(features.join(","))
                    .run_with_capture()?;
            }
            _ => {
                return Err(format!(
                    "Invalid example format: {}, expected 'name' or 'name:features'",
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
/// 3. Auto-discovered features individually + subsets per commit (unconditional)
/// 4. Exact feature sets (when configured)
fn do_feature_matrix(
    sh: &Shell,
    toolchain: Toolchain,
    package: &Package,
    config: &TestConfig,
    cargo_args: &[String],
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Running feature matrix tests in {}", package.name);

    // Test all features.
    rbmt_eprintln!("Testing all features in {}", package.name);
    test_features(sh, toolchain, None, cargo_args, &config.msrv_overrides)?;

    // Test no features.
    rbmt_eprintln!("Testing no features in {}", package.name);
    test_features(sh, toolchain, Some(&[]), cargo_args, &config.msrv_overrides)?;

    // Test each discovered feature in isolation, plus subsets.
    let features: Vec<String> = discover_features(sh, package)?
        .into_iter()
        .filter(|f| !config.exclude_features.contains(f))
        .collect();
    if !features.is_empty() {
        rbmt_eprintln!(
            "Discovered {} feature(s) in {} to test: {:?}",
            features.len(),
            package.name,
            features
        );
        discovered_feature_matrix(
            sh,
            toolchain,
            &features,
            config.sample_strategy,
            cargo_args,
            &config.msrv_overrides,
            summary,
        )?;
    }

    // Test exact feature sets.
    for features in &config.exact_features {
        rbmt_eprintln!("Testing exact feature set in {}: {:?}", package.name, features);
        test_features(
            sh,
            toolchain,
            Some(features.as_slice()),
            cargo_args,
            &config.msrv_overrides,
        )?;
        summary.feature_subsets.push(features.clone());
    }

    Ok(())
}

/// Test auto-discovered features with configurable sampling strategy.
fn discovered_feature_matrix(
    sh: &Shell,
    toolchain: Toolchain,
    features: &[String],
    strategy: SampleStrategy,
    cargo_args: &[String],
    msrv_overrides: &MsrvOverrides,
    summary: &mut PackageSummary,
) -> Result<(), Box<dyn std::error::Error>> {
    // Test each feature individually.
    for feature in features {
        rbmt_eprintln!("Testing individual feature in {}: {}", summary.name, feature);
        test_features(
            sh,
            toolchain,
            Some(std::slice::from_ref(feature)),
            cargo_args,
            msrv_overrides,
        )?;
        summary.individual_features.push(feature.clone());
    }

    // Generate and test feature subsets according to strategy.
    let commit = git_commit_id(sh);
    for subset in strategy.generate_subsets(features, commit) {
        rbmt_eprintln!("Testing feature set in {}: {:?}", summary.name, subset);
        test_features(sh, toolchain, Some(&subset), cargo_args, msrv_overrides)?;
        summary.feature_subsets.push(subset);
    }

    Ok(())
}

/// Detect if a package is attempting to be no-std.
fn is_no_std_package(sh: &Shell, package_dir: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    // Use cargo metadata to find the library target's source path.
    let metadata = rbmt_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
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
        rbmt_eprintln!("{} does not appear to be no-std, skipping test", summary.name);
        return Ok(());
    }

    rbmt_eprintln!(
        "Detected {} as a no-std package, building for target: {}",
        summary.name,
        NO_STD_TARGET
    );
    rbmt_cmd!(sh, "cargo build --target {NO_STD_TARGET} --no-default-features")
        .run_with_capture()?;
    summary.no_std_checked = true;
    Ok(())
}
