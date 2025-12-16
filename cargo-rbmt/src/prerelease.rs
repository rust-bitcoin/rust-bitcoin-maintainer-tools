//! Pre-release readiness checks.

use crate::environment::{get_packages, get_target_dir, quiet_println, CONFIG_FILE_PATH};
use crate::lock::LockFile;
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use serde::Deserialize;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use xshell::Shell;

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
            return Ok(PrereleaseConfig { skip: false });
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.prerelease)
    }
}

/// Run pre-release readiness checks for all packages.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let package_info = get_packages(sh, packages)?;
    quiet_println(&format!(
        "Running pre-release checks on {} packages",
        package_info.len()
    ));

    let mut skipped = Vec::new();

    for (_package_name, package_dir) in &package_info {
        let config = PrereleaseConfig::load(Path::new(package_dir))?;

        if config.skip {
            skipped.push(package_dir);
            quiet_println(&format!(
                "Skipping package: {} (marked as skip)",
                package_dir.display()
            ));
            continue;
        }

        quiet_println(&format!("Checking package: {}", package_dir.display()));

        let _dir = sh.push_dir(package_dir);

        // Run all pre-release checks. Return immediately on first failure.
        if let Err(e) = check_todos(sh) {
            eprintln!(
                "Pre-release check failed for {}: {}",
                package_dir.display(),
                e
            );
            return Err(e);
        }

        if let Err(e) = check_publish(sh) {
            eprintln!(
                "Pre-release check failed for {}: {}",
                package_dir.display(),
                e
            );
            return Err(e);
        }
    }

    quiet_println("All pre-release checks passed");
    Ok(())
}

/// Check for TODO and FIXME comments in source files.
fn check_todos(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Checking for TODO and FIXME comments...");

    let mut todos = Vec::new();
    let src_dir = sh.current_dir().join("src");

    // Recursively walk the src/ directory.
    let mut dirs_to_visit = vec![src_dir];
    while let Some(dir) = dirs_to_visit.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                // Check Rust source files for TODO and FIXME comments.
                let file = fs::File::open(&path)?;
                let reader = BufReader::new(file);

                for (line_num, line) in reader.lines().enumerate() {
                    let line = line?;
                    if line.contains("// TODO")
                        || line.contains("/* TODO")
                        || line.contains("// FIXME")
                        || line.contains("/* FIXME")
                    {
                        todos.push((path.clone(), line_num + 1, line));
                    }
                }
            }
        }
    }

    if !todos.is_empty() {
        eprintln!("\nFound {} TODO/FIXME comment(s):", todos.len());
        for (file, line_num, line) in &todos {
            eprintln!("{}:{}:{}", file.display(), line_num, line.trim());
        }
        return Err(format!("Found {} TODO/FIXME comments", todos.len()).into());
    }

    quiet_println("No TODO or FIXME comments found");
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

    let packages = json["packages"]
        .as_array()
        .ok_or("Missing 'packages' field in cargo metadata")?;

    for package in packages {
        let manifest_path = package["manifest_path"]
            .as_str()
            .ok_or("Missing manifest_path in package")?;

        if manifest_path == current_manifest.to_str().ok_or("Invalid path")? {
            let name = package["name"].as_str().ok_or("Missing name in package")?;

            let version = package["version"]
                .as_str()
                .ok_or("Missing version in package")?;

            return Ok(format!("{}/package/{}-{}", target_dir, name, version));
        }
    }

    Err("Could not find current package in cargo metadata".into())
}
