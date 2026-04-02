//! Integration test tasks for packages with bitcoind-tests or similar test packages.

use std::path::Path;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{discover_features, rbmt_eprintln, Package, PackageManifest};
use crate::rbmt_cmd;

/// Integration-specific configuration, read from `[package.metadata.rbmt.integration]` in `Cargo.toml`.
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
    /// Load integration configuration from `[package.metadata.rbmt.integration]` in the package's `Cargo.toml`.
    fn load(crate_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize, Default)]
        struct RbmtTable {
            #[serde(default)]
            integration: IntegrationConfig,
        }

        let path = crate_dir.join("Cargo.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str::<PackageManifest<RbmtTable>>(&contents)?
            .package
            .metadata
            .rbmt
            .integration)
    }

    /// Get the package name (defaults to "bitcoind-tests").
    fn package_name(&self) -> &str { self.package.as_deref().unwrap_or("bitcoind-tests") }
}

/// Get the package ID by running `cargo pkgid` in the given directory.
fn get_package_id(sh: &Shell, dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let _dir = sh.push_dir(dir);
    let id = rbmt_cmd!(sh, "cargo pkgid").read()?;
    Ok(id.trim().to_string())
}

/// Run integration tests for all crates with integration test packages.
///
/// # Arguments
///
/// * `packages` - Optional filter for specific package names.
pub fn run(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln(&format!("Looking for integration tests in {} crate(s)", packages.len()));

    for package in packages {
        let config = IntegrationConfig::load(Path::new(&package.dir))?;
        let integration_dir = package.dir.join(config.package_name());

        if !integration_dir.exists() {
            continue;
        }

        if !integration_dir.join("Cargo.toml").exists() {
            continue;
        }

        rbmt_eprintln(&format!("Running integration tests for {}", package.name));

        let _dir = sh.push_dir(&integration_dir);

        let integration_package = Package {
            name: config.package_name().to_string(),
            dir: integration_dir.clone(),
            id: get_package_id(sh, &integration_dir)?,
        };
        let available_versions = discover_features(sh, &integration_package)?;
        if available_versions.is_empty() {
            rbmt_eprintln("  No version features found in Cargo.toml");
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
            rbmt_eprintln(&format!("  Testing with version: {}", version));
            rbmt_cmd!(sh, "cargo --locked test --features={version}").run()?;
        }
    }

    Ok(())
}
