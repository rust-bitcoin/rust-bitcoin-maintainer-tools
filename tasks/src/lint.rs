use std::fs;
use xshell::Shell;

use crate::environment::{get_crate_dirs, quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};

/// Lint configuration loaded from contrib/rbmt.toml.
#[derive(Debug, serde::Deserialize)]
struct Config {
    lint: LintConfig,
}

/// Lint-specific configuration.
#[derive(Debug, serde::Deserialize)]
struct LintConfig {
    /// List of crate names that are allowed to have duplicate versions.
    allowed_duplicates: Vec<String>,
}

impl LintConfig {
    /// Load lint configuration from the workspace root.
    fn load(sh: &Shell) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = sh.current_dir().join(CONFIG_FILE_PATH);

        if !config_path.exists() {
            // Return empty config if file doesn't exist.
            return Ok(LintConfig {
                allowed_duplicates: Vec::new(),
            });
        }

        let contents = fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.lint)
    }
}

/// Run the lint task.
pub fn run(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;
    quiet_println("Running lint task...");

    lint_workspace(sh)?;
    lint_crates(sh)?;
    check_duplicate_deps(sh)?;

    quiet_println("Lint task completed successfully");
    Ok(())
}

/// Lint the workspace with clippy.
fn lint_workspace(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Linting workspace...");

    // Run clippy on workspace with all features.
    quiet_cmd!(
        sh,
        "cargo clippy --workspace --all-targets --all-features --keep-going"
    )
    .args(&["--", "-D", "warnings"])
    .run()?;

    // Run clippy on workspace without features.
    quiet_cmd!(sh, "cargo clippy --workspace --all-targets --keep-going")
        .args(&["--", "-D", "warnings"])
        .run()?;

    Ok(())
}

/// Run extra crate-specific lints.
///
/// # Why run at the crate level?
///
/// When running `cargo clippy --workspace --no-default-features`, cargo resolves
/// features across the entire workspace, which can enable features through dependencies
/// even when a crate's own default features are disabled. Running clippy on each crate
/// individually ensures that each crate truly compiles and passes lints with only its
/// explicitly enabled features.
fn lint_crates(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running crate-specific lints...");

    let crate_dirs = get_crate_dirs(sh)?;
    quiet_println(&format!("Found crates: {}", crate_dirs.join(", ")));

    for crate_dir in crate_dirs {
        // Returns a RAII guard which reverts the working directory to the old value when dropped.
        let _old_dir = sh.push_dir(&crate_dir);

        // Run clippy without default features.
        quiet_cmd!(
            sh,
            "cargo clippy --all-targets --no-default-features --keep-going"
        )
        .args(&["--", "-D", "warnings"])
        .run()?;
    }

    Ok(())
}

/// Check for duplicate dependencies.
fn check_duplicate_deps(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Checking for duplicate dependencies...");

    let config = LintConfig::load(sh)?;
    let allowed_duplicates = &config.allowed_duplicates;

    // Run cargo tree to find duplicates.
    let output = quiet_cmd!(sh, "cargo tree --target=all --all-features --duplicates")
        .ignore_status()
        .read()?;

    let duplicates: Vec<&str> = output
        .lines()
        // Filter out non crate names.
        .filter(|line| line.chars().next().is_some_and(|c| c.is_alphanumeric()))
        // Filter out whitelisted crates.
        .filter(|line| {
            !allowed_duplicates
                .iter()
                .any(|allowed| line.contains(allowed))
        })
        .collect();

    if !duplicates.is_empty() {
        // Show full tree for context.
        quiet_cmd!(sh, "cargo tree --target=all --all-features --duplicates").run()?;
        eprintln!("Error: Found duplicate dependencies in workspace!");
        for dup in &duplicates {
            eprintln!("  {}", dup);
        }
        return Err("Dependency tree contains duplicates".into());
    }

    quiet_println("No duplicate dependencies found");
    Ok(())
}
