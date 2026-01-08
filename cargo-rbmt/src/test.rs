//! Build and test tasks with feature matrix testing.
//!
//! `cargo build` runs before `cargo test` throughout this module to try
//! and catch any issues involving `cfg(test)` somehow gating required code.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{get_packages, quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};

/// Conventinal feature flags used across rust-bitcoin crates.
#[derive(Debug, Clone, Copy)]
enum FeatureFlag {
    /// Enable the standard library.
    Std,
    /// Legacy feature to disable standard library.
    NoStd,
}

impl FeatureFlag {
    /// Get the feature string for this flag.
    fn as_str(self) -> &'static str {
        match self {
            Self::Std => "std",
            Self::NoStd => "no-std",
        }
    }
}

impl fmt::Display for FeatureFlag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "{}", self.as_str()) }
}

impl AsRef<str> for FeatureFlag {
    fn as_ref(&self) -> &str { self.as_str() }
}

impl AsRef<OsStr> for FeatureFlag {
    fn as_ref(&self) -> &OsStr { OsStr::new(self.as_str()) }
}

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

    /// List of individual features to test with the conventional `std` feature enabled.
    /// Automatically tests feature combinations, alone with `std`, all pairs, and all together.
    ///
    /// # Examples
    ///
    /// `["serde", "rand"]` tests `std+serde`, `std+rand`, `std+serde+rand`.
    features_with_std: Vec<String>,

    /// List of individual features to test without the `std` feature.
    /// Automatically tests features combinations, each feature alone,
    /// all pairs, and all together.
    ///
    /// # Examples
    ///
    /// `["serde", "rand"]` tests `serde`, `rand`, `serde+rand`.
    features_without_std: Vec<String>,

    /// Exact feature combinations to test.
    /// Use for crates that don't follow the conventional `std` feature pattern.
    /// Each inner vector is a list of features to test together. There is
    /// no automatic combinations of features tests.
    ///
    /// # Examples
    ///
    /// `[["serde", "rand"], ["rand"]]` tests exactly those two combinations.
    exact_features: Vec<Vec<String>>,

    /// List of individual features to test with the `no-std` feature enabled.
    /// Only use if your crate has an explicit `no-std` feature (rust-miniscript pattern).
    /// Automatically tests each feature alone with `no-std`, all pairs, and all together.
    ///
    /// # Examples
    ///
    /// `["serde", "rand"]` tests `no-std+serde`, `no-std+serde`, `no-std+serde+rand`.
    features_with_no_std: Vec<String>,
}

impl TestConfig {
    /// Load test configuration from a crate directory.
    fn load(crate_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = crate_dir.join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            // Return empty config if file doesn't exist.
            return Ok(Self {
                examples: Vec::new(),
                features_with_std: Vec::new(),
                features_without_std: Vec::new(),
                exact_features: Vec::new(),
                features_with_no_std: Vec::new(),
            });
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.test)
    }
}

/// Run build and test for all crates with the specified toolchain.
pub fn run(
    sh: &Shell,
    toolchain: Toolchain,
    no_debug_assertions: bool,
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let package_info = get_packages(sh, packages)?;
    quiet_println(&format!("Testing {} crates", package_info.len()));

    // Configure RUSTFLAGS for debug assertions.
    let _env = sh.push_env(
        "RUSTFLAGS",
        if no_debug_assertions { "-C debug-assertions=off" } else { "-C debug-assertions=on" },
    );

    for (_package_name, package_dir) in &package_info {
        quiet_println(&format!("Testing crate: {}", package_dir.display()));

        let _dir = sh.push_dir(package_dir);
        // Check the package's MSRV, not the workspace root.
        check_toolchain(sh, toolchain)?;
        let config = TestConfig::load(Path::new(package_dir))?;

        do_test(sh, &config)?;
        do_feature_matrix(sh, &config)?;
        do_no_std_check(sh, Path::new(package_dir))?;
    }

    Ok(())
}

/// Run basic build, test, and examples.
fn do_test(sh: &Shell, config: &TestConfig) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running basic tests");

    // Basic build and test.
    quiet_cmd!(sh, "cargo --locked build").run()?;
    quiet_cmd!(sh, "cargo --locked test").run()?;

    // Run examples.
    for example in &config.examples {
        let parts: Vec<&str> = example.split(':').collect();

        match parts.len() {
            1 => {
                // Format: "name" - run with default features.
                let name = parts[0];
                quiet_cmd!(sh, "cargo --locked run --example {name}").run()?;
            }
            2 => {
                let name = parts[0];
                let features = parts[1];

                if features == "-" {
                    // Format: "name:-" - run with no-default-features.
                    quiet_cmd!(sh, "cargo --locked run --no-default-features --example {name}")
                        .run()?;
                } else {
                    // Format: "name:features" - run with specific features.
                    quiet_cmd!(sh, "cargo --locked run --example {name} --features={features}")
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
fn do_feature_matrix(sh: &Shell, config: &TestConfig) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running feature matrix tests");

    // Handle exact features (for unusual crates).
    if !config.exact_features.is_empty() {
        for features in &config.exact_features {
            let features_str = features.join(" ");
            quiet_println(&format!("Testing exact features: {}", features_str));
            quiet_cmd!(sh, "cargo --locked build --no-default-features --features={features_str}")
                .run()?;
            quiet_cmd!(sh, "cargo --locked test --no-default-features --features={features_str}")
                .run()?;
        }
        return Ok(());
    }

    // Handle no-std pattern (rust-miniscript).
    if config.features_with_no_std.is_empty() {
        quiet_println("Testing no-default-features");
        quiet_cmd!(sh, "cargo --locked build --no-default-features").run()?;
        quiet_cmd!(sh, "cargo --locked test --no-default-features").run()?;
    } else {
        let no_std = FeatureFlag::NoStd;
        quiet_println("Testing no-std");
        quiet_cmd!(sh, "cargo --locked build --no-default-features --features={no_std}").run()?;
        quiet_cmd!(sh, "cargo --locked test --no-default-features --features={no_std}").run()?;

        loop_features(sh, Some(FeatureFlag::NoStd), &config.features_with_no_std)?;
    }

    // Test all features.
    quiet_println("Testing all-features");
    quiet_cmd!(sh, "cargo --locked build --all-features").run()?;
    quiet_cmd!(sh, "cargo --locked test --all-features").run()?;

    // Test features with std.
    if !config.features_with_std.is_empty() {
        loop_features(sh, Some(FeatureFlag::Std), &config.features_with_std)?;
    }

    // Test features without std.
    if !config.features_without_std.is_empty() {
        loop_features(sh, None, &config.features_without_std)?;
    }

    Ok(())
}

/// Test each feature individually and all combinations of two features.
///
/// This implements three feature matrix testing strategies:
/// 1. All features together (base feature + all test features).
/// 2. Each feature individually (base feature + one test feature).
/// 3. All unique pairs of test features (base feature + two test features).
///
/// The pair testing catches feature interaction bugs (where two features work
/// independently, but conflict when combined) while keeping test time manageable.
///
/// # Parameters
///
/// * `base` - Optional base feature that is always included (e.g., `Some(FeatureFlag::Std)`).
/// * `features` - Features to test in combination.
fn loop_features<S: AsRef<str>>(
    sh: &Shell,
    base: Option<FeatureFlag>,
    features: &[S],
) -> Result<(), Box<dyn std::error::Error>> {
    // Helper to combine base flag and features into a feature flag string.
    fn combine_features<S: AsRef<str>>(base: Option<FeatureFlag>, additional: &[S]) -> String {
        match base {
            Some(flag) => std::iter::once(flag.as_ref())
                .chain(additional.iter().map(std::convert::AsRef::as_ref))
                .collect::<Vec<_>>()
                .join(" "),
            None =>
                additional.iter().map(std::convert::AsRef::as_ref).collect::<Vec<_>>().join(" "),
        }
    }

    // Test all features together.
    let all_features = combine_features(base, features);
    quiet_println(&format!("Testing features: {}", all_features));
    quiet_cmd!(sh, "cargo --locked build --no-default-features --features={all_features}").run()?;
    quiet_cmd!(sh, "cargo --locked test --no-default-features --features={all_features}").run()?;

    // Test each feature individually and all pairs (only if more than one feature).
    if features.len() > 1 {
        for i in 0..features.len() {
            let feature_combo = combine_features(base, &features[i..=i]);
            quiet_println(&format!("Testing features: {}", feature_combo));
            quiet_cmd!(sh, "cargo --locked build --no-default-features --features={feature_combo}")
                .run()?;
            quiet_cmd!(sh, "cargo --locked test --no-default-features --features={feature_combo}")
                .run()?;

            // Test all pairs with features[i].
            for j in (i + 1)..features.len() {
                let pair = [&features[i], &features[j]];
                let feature_combo = combine_features(base, &pair);
                quiet_println(&format!("Testing features: {}", feature_combo));
                quiet_cmd!(
                    sh,
                    "cargo --locked build --no-default-features --features={feature_combo}"
                )
                .run()?;
                quiet_cmd!(
                    sh,
                    "cargo --locked test --no-default-features --features={feature_combo}"
                )
                .run()?;
            }
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

    quiet_println(&format!("Detected no-std package, checking with target: {}", NO_STD_TARGET));
    quiet_cmd!(sh, "cargo check --target {NO_STD_TARGET} --no-default-features").run()?;
    quiet_println("no-std check passed!");
    Ok(())
}
