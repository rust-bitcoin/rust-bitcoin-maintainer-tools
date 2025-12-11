//! Build and test tasks with feature matrix testing.

use crate::environment::{get_crate_dirs, quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use serde::Deserialize;
use std::path::Path;
use xshell::Shell;

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
            return Ok(TestConfig {
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
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, toolchain)?;

    let crate_dirs = get_crate_dirs(sh, packages)?;
    quiet_println(&format!("Testing {} crates", crate_dirs.len()));

    for crate_dir in &crate_dirs {
        quiet_println(&format!("Testing crate: {}", crate_dir));

        let _dir = sh.push_dir(crate_dir);
        let config = TestConfig::load(Path::new(crate_dir))?;

        do_test(sh, &config)?;
        do_feature_matrix(sh, &config)?;
    }

    Ok(())
}

/// Run basic build, test, and examples.
fn do_test(sh: &Shell, config: &TestConfig) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running basic tests");

    // Basic build and test.
    quiet_cmd!(sh, "cargo build").run()?;
    quiet_cmd!(sh, "cargo test").run()?;

    // Run examples.
    for example in &config.examples {
        let parts: Vec<&str> = example.split(':').collect();

        match parts.len() {
            1 => {
                // Format: "name" - run with default features.
                let name = parts[0];
                quiet_cmd!(sh, "cargo run --locked --example {name}").run()?;
            }
            2 => {
                let name = parts[0];
                let features = parts[1];

                if features == "-" {
                    // Format: "name:-" - run with no-default-features.
                    quiet_cmd!(
                        sh,
                        "cargo run --locked --no-default-features --example {name}"
                    )
                    .run()?;
                } else {
                    // Format: "name:features" - run with specific features.
                    quiet_cmd!(
                        sh,
                        "cargo run --locked --example {name} --features={features}"
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
fn do_feature_matrix(sh: &Shell, config: &TestConfig) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running feature matrix tests");

    // Handle exact features (for unusual crates).
    if !config.exact_features.is_empty() {
        for features in &config.exact_features {
            let features_str = features.join(" ");
            quiet_println(&format!("Testing exact features: {}", features_str));
            quiet_cmd!(
                sh,
                "cargo build --no-default-features --features={features_str}"
            )
            .run()?;
            quiet_cmd!(
                sh,
                "cargo test --no-default-features --features={features_str}"
            )
            .run()?;
        }
        return Ok(());
    }

    // Handle no-std pattern (rust-miniscript).
    if !config.features_with_no_std.is_empty() {
        quiet_println("Testing no-std");
        quiet_cmd!(sh, "cargo build --no-default-features --features=no-std").run()?;
        quiet_cmd!(sh, "cargo test --no-default-features --features=no-std").run()?;

        loop_features(sh, "no-std", &config.features_with_no_std)?;
    } else {
        quiet_println("Testing no-default-features");
        quiet_cmd!(sh, "cargo build --no-default-features").run()?;
        quiet_cmd!(sh, "cargo test --no-default-features").run()?;
    }

    // Test all features.
    quiet_println("Testing all-features");
    quiet_cmd!(sh, "cargo build --all-features").run()?;
    quiet_cmd!(sh, "cargo test --all-features").run()?;

    // Test features with std.
    if !config.features_with_std.is_empty() {
        loop_features(sh, "std", &config.features_with_std)?;
    }

    // Test features without std.
    if !config.features_without_std.is_empty() {
        loop_features(sh, "", &config.features_without_std)?;
    }

    Ok(())
}

/// Test each feature individually and all combinations of two features.
///
/// This implements three feature matrix testing strategies.
/// 1. All features together.
/// 2. Each feature individually (only if more than one feature).
/// 3. All unique pairs of features.
///
/// The pair testing catches feature interaction bugs (where two features work
/// independently, but conflict when combined) while keeping test time manageable.
fn loop_features(
    sh: &Shell,
    base: &str,
    features: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let base_flag = if base.is_empty() {
        String::new()
    } else {
        format!("{} ", base)
    };

    // Test all features together.
    let all_features = format!("{}{}", base_flag, features.join(" "));
    quiet_println(&format!("Testing features: {}", all_features.trim()));
    quiet_cmd!(
        sh,
        "cargo build --no-default-features --features={all_features}"
    )
    .run()?;
    quiet_cmd!(
        sh,
        "cargo test --no-default-features --features={all_features}"
    )
    .run()?;

    // Test each feature individually and all pairs (only if more than one feature).
    if features.len() > 1 {
        for i in 0..features.len() {
            let feature_combo = format!("{}{}", base_flag, features[i]);
            quiet_println(&format!("Testing features: {}", feature_combo.trim()));
            quiet_cmd!(
                sh,
                "cargo build --no-default-features --features={feature_combo}"
            )
            .run()?;
            quiet_cmd!(
                sh,
                "cargo test --no-default-features --features={feature_combo}"
            )
            .run()?;

            // Test all pairs with features[i].
            for j in (i + 1)..features.len() {
                let feature_combo = format!("{}{} {}", base_flag, features[i], features[j]);
                quiet_println(&format!("Testing features: {}", feature_combo.trim()));
                quiet_cmd!(
                    sh,
                    "cargo build --no-default-features --features={feature_combo}"
                )
                .run()?;
                quiet_cmd!(
                    sh,
                    "cargo test --no-default-features --features={feature_combo}"
                )
                .run()?;
            }
        }
    }

    Ok(())
}
