//! Pre-release readiness checks.

use crate::environment::{get_crate_dirs, quiet_println, CONFIG_FILE_PATH};
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
    let package_dirs = get_crate_dirs(sh, packages)?;
    quiet_println(&format!(
        "Running pre-release checks on {} packages",
        package_dirs.len()
    ));

    let mut skipped = Vec::new();

    for package_dir in &package_dirs {
        let config = PrereleaseConfig::load(Path::new(package_dir))?;

        if config.skip {
            skipped.push(package_dir.as_str());
            quiet_println(&format!(
                "Skipping package: {} (marked as skip)",
                package_dir
            ));
            continue;
        }

        quiet_println(&format!("Checking package: {}", package_dir));

        let _dir = sh.push_dir(package_dir);

        // Run all pre-release checks. Return immediately on first failure.
        if let Err(e) = check_todos(sh) {
            eprintln!("Pre-release check failed for {}: {}", package_dir, e);
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
