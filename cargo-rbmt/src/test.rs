//! Build and test tasks with feature matrix testing.
//!
//! `cargo build` runs before `cargo test` throughout this module to try
//! and catch any issues involving `cfg(test)` somehow gating required code.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::Deserialize;
use xshell::{Cmd, Shell};

use crate::environment::{
    discover_features, git_commit_id, quiet_println, Package, CONFIG_FILE_PATH,
};
use crate::quiet_cmd;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Test configuration loaded from rbmt.toml.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Config {
    test: TestConfig,
}

/// Test-specific configuration.
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
    /// ```
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
    /// Load test configuration from a crate directory.
    fn load(crate_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = crate_dir.join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.test)
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
pub fn run(
    sh: &Shell,
    toolchain: Toolchain,
    no_debug_assertions: bool,
    release: bool,
    packages: &[Package],
) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println(&format!("Testing {} crates", packages.len()));

    // Configure RUSTFLAGS for debug assertions.
    let _env = sh.push_env(
        "RUSTFLAGS",
        if no_debug_assertions { "-C debug-assertions=off" } else { "-C debug-assertions=on" },
    );

    for package in packages {
        let (_, package_dir) = package;
        quiet_println(&format!("Testing crate: {}", package_dir.display()));

        let _dir = sh.push_dir(package_dir);
        // prepare_toolchain is called per-package because MSRV is read from
        // each package's Cargo.toml individually rather than the workspace root.
        prepare_toolchain(sh, toolchain)?;
        let config = TestConfig::load(Path::new(package_dir))?;
        let release = release || config.release;

        do_test(sh, &config, release)?;
        do_feature_matrix(sh, package, &config, release)?;
        do_no_std_check(sh, Path::new(package_dir))?;
    }

    Ok(())
}

/// Run basic build, test, and examples.
fn do_test(
    sh: &Shell,
    config: &TestConfig,
    release: bool,
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
        sampled_feature_matrix(sh, &features, release)?;
    }

    // Test exact feature sets.
    for features in &config.exact_features {
        test_features(sh, features, release)?;
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
fn do_no_std_check(sh: &Shell, package_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    const NO_STD_TARGET: &str = "thumbv7m-none-eabi";
    if !is_no_std_package(sh, package_dir)? {
        return Ok(());
    }

    quiet_println(&format!("Detected no-std package, building for target: {}", NO_STD_TARGET));
    quiet_cmd!(sh, "cargo build --target {NO_STD_TARGET} --no-default-features").run()?;
    quiet_println("no-std build passed!");
    Ok(())
}
