use std::fs;

use xshell::Shell;

use crate::environment::{get_packages, quiet_println, CONFIG_FILE_PATH};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};

/// Lint configuration loaded from rbmt.toml.
#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
struct Config {
    lint: LintConfig,
}

/// Lint-specific configuration.
#[derive(Debug, serde::Deserialize, Default)]
#[serde(default)]
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
            return Ok(Self { allowed_duplicates: Vec::new() });
        }

        let contents = fs::read_to_string(&config_path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config.lint)
    }
}

/// Run the lint task.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;
    quiet_println("Running lint task...");

    lint_workspace(sh)?;
    lint_packages(sh, packages)?;
    check_duplicate_deps(sh)?;
    check_clippy_toml_msrv(sh, packages)?;

    quiet_println("Lint task completed successfully");
    Ok(())
}

/// Lint the workspace with clippy.
fn lint_workspace(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Linting workspace...");

    // Run clippy on workspace with all features.
    quiet_cmd!(sh, "cargo --locked clippy --workspace --all-targets --all-features --keep-going")
        .args(&["--", "-D", "warnings"])
        .run()?;

    // Run clippy on workspace without features.
    quiet_cmd!(sh, "cargo --locked clippy --workspace --all-targets --keep-going")
        .args(&["--", "-D", "warnings"])
        .run()?;

    Ok(())
}

/// Run extra package-specific lints.
///
/// # Why run at the package level?
///
/// When running `cargo clippy --workspace --no-default-features`, cargo resolves
/// features across the entire workspace, which can enable features through dependencies
/// even when a package's own default features are disabled. Running clippy on each package
/// individually ensures that each package truly compiles and passes lints with only its
/// explicitly enabled features.
fn lint_packages(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running package-specific lints...");

    let package_info = get_packages(sh, packages)?;
    let package_names: Vec<_> = package_info.iter().map(|(name, _)| name.as_str()).collect();
    quiet_println(&format!("Found crates: {}", package_names.join(", ")));

    for (_package_name, package_dir) in package_info {
        // Returns a RAII guard which reverts the working directory to the old value when dropped.
        let _old_dir = sh.push_dir(&package_dir);

        // Run clippy without default features.
        quiet_cmd!(sh, "cargo --locked clippy --all-targets --no-default-features --keep-going")
            .args(&["--", "-D", "warnings"])
            .run()?;
    }

    Ok(())
}

/// Check for duplicate dependencies.
///
/// # Why run at the workspace level?
///
/// Running at the workspace level is more strict than per-package. If a package
/// has duplicates, the workspace has duplicates (subset relationship). The
/// workspace check catches all duplicates, both within-package and cross-package.
fn check_duplicate_deps(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Checking for duplicate dependencies...");

    let config = LintConfig::load(sh)?;
    let allowed_duplicates = &config.allowed_duplicates;

    // Run cargo tree to find duplicates.
    let output = quiet_cmd!(sh, "cargo --locked tree --target=all --all-features --duplicates")
        .ignore_status()
        .read()?;

    let duplicates: Vec<&str> = output
        .lines()
        // Filter out non crate names.
        .filter(|line| line.chars().next().is_some_and(char::is_alphanumeric))
        // Filter out whitelisted crates.
        .filter(|line| !allowed_duplicates.iter().any(|allowed| line.contains(allowed)))
        .collect();

    if !duplicates.is_empty() {
        // Show full tree for context.
        quiet_cmd!(sh, "cargo --locked tree --target=all --all-features --duplicates").run()?;
        eprintln!("Error: Found duplicate dependencies in workspace!");
        for dup in &duplicates {
            eprintln!("  {}", dup);
        }
        return Err("Dependency tree contains duplicates".into());
    }

    quiet_println("No duplicate dependencies found");
    Ok(())
}

/// Check for deprecated clippy.toml MSRV settings.
///
/// The bitcoin ecosystem has moved to Rust 1.74+ and should use Cargo.toml
/// package.rust-version instead of clippy.toml msrv settings.
fn check_clippy_toml_msrv(
    sh: &Shell,
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    const CLIPPY_CONFIG_FILES: &[&str] = &["clippy.toml", ".clippy.toml"];

    quiet_println("Checking for deprecated clippy.toml MSRV settings...");

    let mut clippy_files = Vec::new();

    // Check workspace root.
    let workspace_root = sh.current_dir();
    for filename in CLIPPY_CONFIG_FILES {
        let path = workspace_root.join(filename);
        if path.exists() {
            clippy_files.push(path);
        }
    }

    // Check each package.
    let package_info = get_packages(sh, packages)?;
    for (_package_name, package_dir) in package_info {
        for filename in CLIPPY_CONFIG_FILES {
            let path = package_dir.join(filename);
            if path.exists() {
                clippy_files.push(path);
            }
        }
    }

    // Check each clippy file for the msrv setting.
    let mut problematic_files = Vec::new();
    for path in clippy_files {
        let contents = fs::read_to_string(&path)?;
        let config: toml::Value = toml::from_str(&contents)?;

        if config.get("msrv").is_some() {
            problematic_files.push(path.display().to_string());
        }
    }

    if !problematic_files.is_empty() {
        eprintln!(
            "\nError: Found MSRV in clippy.toml, use Cargo.toml package.rust-version instead:"
        );
        for file in &problematic_files {
            eprintln!("  {}", file);
        }
        return Err("MSRV should be specified in Cargo.toml, not clippy.toml".into());
    }

    quiet_println("No deprecated clippy.toml MSRV settings found");
    Ok(())
}
