//! Integration test tasks for packages with bitcoind-tests or similar test packages.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use xshell::{cmd, Shell};

use crate::environment::{get_packages, quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;

/// Integration test configuration loaded from rbmt.toml.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Config {
    integration: IntegrationConfig,
}

/// Integration-specific configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct IntegrationConfig {
    /// Package name containing integration tests (defaults to "bitcoind-tests").
    package: Option<String>,

    /// Bitcoind versions to test (runs each individually).
    /// If not specified, discovers all version features from Cargo.toml.
    ///
    /// # Examples
    ///
    /// `["29_0", "28_2", "27_2"]`
    versions: Option<Vec<String>>,
}

impl IntegrationConfig {
    /// Load integration configuration from a crate directory.
    fn load(crate_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = crate_dir.join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.integration)
    }

    /// Get the package name (defaults to "bitcoind-tests").
    fn package_name(&self) -> &str { self.package.as_deref().unwrap_or("bitcoind-tests") }
}

/// Run integration tests for all crates with integration test packages.
///
/// # Arguments
///
/// * `packages` - Optional filter for specific package names.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let package_info = get_packages(sh, packages)?;
    quiet_println(&format!("Looking for integration tests in {} crate(s)", package_info.len()));

    for (_package_name, package_dir) in &package_info {
        let config = IntegrationConfig::load(Path::new(package_dir))?;
        let integration_dir = PathBuf::from(package_dir).join(config.package_name());

        if !integration_dir.exists() {
            continue;
        }

        if !integration_dir.join("Cargo.toml").exists() {
            continue;
        }

        quiet_println(&format!("Running integration tests for crate: {}", package_dir.display()));

        let _dir = sh.push_dir(&integration_dir);

        let available_versions = discover_version_features(sh, &integration_dir)?;
        if available_versions.is_empty() {
            quiet_println("  No version features found in Cargo.toml");
            continue;
        }

        let versions_to_test: Vec<String> = if let Some(config_versions) = &config.versions {
            // Filter available versions by config.
            let mut filtered = Vec::new();
            for requested in config_versions {
                if available_versions.contains(requested) {
                    filtered.push(requested.clone());
                } else {
                    return Err(format!(
                        "Requested version '{}' not found in available versions: {}",
                        requested,
                        available_versions.join(", ")
                    )
                    .into());
                }
            }
            filtered
        } else {
            // No config, test all available versions.
            available_versions
        };

        // Run tests for each version.
        for version in &versions_to_test {
            quiet_println(&format!("  Testing with version: {}", version));
            quiet_cmd!(sh, "cargo --locked test --features={version}").run()?;
        }
    }

    Ok(())
}

/// Discover all features from the integration package using cargo metadata.
fn discover_version_features(
    sh: &Shell,
    integration_dir: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let _dir = sh.push_dir(integration_dir);
    let metadata = cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let mut features = Vec::new();

    // Find the package in the metadata and extract its features.
    if let Some(packages) = json["packages"].as_array() {
        // Should only be one package since we're in the integration test directory.
        if let Some(package) = packages.first() {
            if let Some(package_features) = package["features"].as_object() {
                for feature_name in package_features.keys() {
                    features.push(feature_name.clone());
                }
            }
        }
    }

    // Sort for consistent output.
    features.sort();

    Ok(features)
}
