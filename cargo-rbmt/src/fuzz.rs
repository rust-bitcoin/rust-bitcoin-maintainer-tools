//! Fuzz test tasks for workspaces with honggfuzz fuzz targets.

use std::path::Path;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;

/// Default package name for fuzz targets.
const FUZZ_PACKAGE: &str = "fuzz";

/// Fuzz configuration loaded from rbmt.toml.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Config {
    fuzz: FuzzConfig,
}

/// Fuzz-specific configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FuzzConfig {
    /// Package name containing fuzz targets (defaults to [`FUZZ_PACKAGE`]).
    package: Option<String>,
}

impl FuzzConfig {
    /// Load fuzz configuration from workspace root.
    fn load(workspace_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = workspace_root.join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.fuzz)
    }

    /// Get the package name (defaults to [`FUZZ_PACKAGE`]).
    fn package_name(&self) -> &str { self.package.as_deref().unwrap_or(FUZZ_PACKAGE) }
}

/// Discover all fuzz targets using cargo metadata.
///
/// Targets are discovered by querying cargo metadata for all binary targets
/// in the specified fuzz package.
fn discover_fuzz_targets(
    sh: &Shell,
    package_name: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let metadata = quiet_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let mut targets = Vec::new();

    // Find binary targets in the specified fuzz package.
    if let Some(packages) = json["packages"].as_array() {
        for package in packages {
            if package["name"].as_str() == Some(package_name) {
                if let Some(package_targets) = package["targets"].as_array() {
                    for target in package_targets {
                        // Filter for binary targets only.
                        let Some(kinds) = target["kind"].as_array() else {
                            continue;
                        };
                        let Some(name) = target["name"].as_str() else {
                            continue;
                        };

                        if kinds.iter().any(|k| k.as_str() == Some("bin")) {
                            targets.push(name.to_string());
                        }
                    }
                }
                break; // Found the package, no need to continue.
            }
        }
    }

    // Sort for consistent output.
    targets.sort();

    Ok(targets)
}

/// List discovered fuzz targets.
pub fn list(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = sh.current_dir();
    let config = FuzzConfig::load(&workspace_root)?;
    let package_name = config.package_name();

    let targets = discover_fuzz_targets(sh, package_name)?;

    if targets.is_empty() {
        quiet_println("No fuzz targets found");
    } else {
        for target in targets {
            println!("{}", target);
        }
    }

    Ok(())
}

/// Run fuzz tests for the workspace.
pub fn run(_sh: &Shell) { quiet_println("Fuzz execution not yet implemented"); }
