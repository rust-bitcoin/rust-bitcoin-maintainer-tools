//! Pre-release readiness checks.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{get_packages, get_target_dir, quiet_println, CONFIG_FILE_PATH};
use crate::lock::LockFile;
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};

/// Pre-release configuration loaded from rbmt.toml.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Config {
    prerelease: PrereleaseConfig,
}

/// Pre-release-specific configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct PrereleaseConfig {
    /// If true, opt-out of pre-release checks for this package.
    skip: bool,
}

impl PrereleaseConfig {
    /// Load pre-release configuration from a package directory.
    fn load(package_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = package_dir.join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            // Return default config (skip = false) if file doesn't exist.
            return Ok(Self { skip: false });
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.prerelease)
    }
}

/// Run pre-release readiness checks for all packages.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let package_info = get_packages(sh, packages)?;
    quiet_println(&format!("Running pre-release checks on {} packages", package_info.len()));

    let mut skipped = Vec::new();

    for (_package_name, package_dir) in &package_info {
        let config = PrereleaseConfig::load(Path::new(package_dir))?;

        if config.skip {
            skipped.push(package_dir);
            quiet_println(&format!("Skipping package: {} (marked as skip)", package_dir.display()));
            continue;
        }

        quiet_println(&format!("Checking package: {}", package_dir.display()));

        let _dir = sh.push_dir(package_dir);

        // Run all pre-release checks. Return immediately on first failure.
        if let Err(e) = check_todos(sh) {
            eprintln!("Pre-release check failed for {}: {}", package_dir.display(), e);
            return Err(e);
        }

        if let Err(e) = check_publish(sh) {
            eprintln!("Pre-release check failed for {}: {}", package_dir.display(), e);
            return Err(e);
        }
    }

    quiet_println("All pre-release checks passed");
    Ok(())
}

// Things which should be patched up before release.
const TODOS: &[&str] = &["// TODO", "/* TODO", "// FIXME", "/* FIXME", "\"TBD\""];
// Things which are banned and can't be released.
const NONOS: &[&str] = &["doc_auto_cfg"];

/// Grep source code for TODO, FIXME, TBD, and `doc_auto_cfg`.
fn check_todos(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Greping source for todos and nonos...");

    // Recursively walk the src/ directory.
    let mut issues = Vec::new();
    let mut dirs_to_visit = vec![sh.current_dir().join("src")];
    while let Some(dir) = dirs_to_visit.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let file = fs::File::open(&path)?;
                let reader = BufReader::new(file);

                for (line_num, line) in reader.lines().enumerate() {
                    let line = line?;
                    if TODOS.iter().any(|pattern| line.contains(pattern))
                        || NONOS.iter().any(|pattern| line.contains(pattern))
                    {
                        issues.push((path.clone(), line_num, line));
                    }
                }
            }
        }
    }

    if !issues.is_empty() {
        eprintln!("Found {} pre-release issue(s):", issues.len());
        for (file, line_num, line) in &issues {
            eprintln!("{}:{}:{}", file.display(), line_num, line.trim());
        }
        return Err(format!("Found {} pre-release issues", issues.len()).into());
    }

    quiet_println("No pre-release issues found");
    Ok(())
}

/// Check that the package can be published.
///
/// A package may work with local path dependencies, but fail when published
/// because the version specifications don't match the published versions
/// or don't resolve correctly. This function tests the package with minimal
/// dependency versions attemting to catch compatibility issues.
fn check_publish(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure we have nightly toolchain for minimal versions testing.
    check_toolchain(sh, Toolchain::Nightly)?;

    quiet_cmd!(sh, "cargo publish --dry-run").run()?;
    let package_dir = get_publish_dir(sh)?;

    let _dir = sh.push_dir(&package_dir);
    quiet_println(&format!("Testing publish package: {}", package_dir));
    LockFile::Minimal.derive(sh)?;
    quiet_cmd!(sh, "cargo test --all-features --all-targets --locked").run()?;

    quiet_println("Publish tests passed");
    Ok(())
}

/// Get the path to the publish directory for the current package from cargo metadata.
fn get_publish_dir(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let target_dir = get_target_dir(sh)?;

    // Find the package that matches the current directory.
    let metadata = quiet_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;
    let current_dir = sh.current_dir();
    let current_manifest = current_dir.join("Cargo.toml");

    let packages =
        json["packages"].as_array().ok_or("Missing 'packages' field in cargo metadata")?;

    for package in packages {
        let manifest_path =
            package["manifest_path"].as_str().ok_or("Missing manifest_path in package")?;

        if manifest_path == current_manifest.to_str().ok_or("Invalid path")? {
            let name = package["name"].as_str().ok_or("Missing name in package")?;

            let version = package["version"].as_str().ok_or("Missing version in package")?;

            return Ok(format!("{}/package/{}-{}", target_dir, name, version));
        }
    }

    Err("Could not find current package in cargo metadata".into())
}
