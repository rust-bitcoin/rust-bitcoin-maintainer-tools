use std::env;
use std::path::PathBuf;

use xshell::Shell;

/// Environment variable to control output verbosity.
/// Set to "quiet" to suppress informational messages and reduce cargo output.
/// Any other value (or unset) defaults to verbose mode.
const LOG_LEVEL_ENV_VAR: &str = "RBMT_LOG_LEVEL";

/// Path to the RBMT configuration file relative to workspace/crate root.
pub const CONFIG_FILE_PATH: &str = "rbmt.toml";

/// Check if we're in quiet mode via environment variable.
pub fn is_quiet_mode() -> bool { env::var(LOG_LEVEL_ENV_VAR).is_ok_and(|v| v == "quiet") }

/// Helper macro to create commands that respect quiet mode.
#[macro_export]
macro_rules! quiet_cmd {
    ($sh:expr, $($arg:tt)*) => {{
        let mut cmd = xshell::cmd!($sh, $($arg)*);
        if $crate::environment::is_quiet_mode() {
            cmd = cmd.quiet();
        }
        cmd
    }};
}

/// Print a message to stderr unless in quiet mode.
pub fn quiet_println(msg: &str) {
    if !is_quiet_mode() {
        eprintln!("{}", msg);
    }
}

/// Configure shell log level and output verbosity.
/// Sets cargo output verbosity based on `LOG_LEVEL_ENV_VAR`.
pub fn configure_log_level(sh: &Shell) {
    if is_quiet_mode() {
        sh.set_var("CARGO_TERM_VERBOSE", "false");
        sh.set_var("CARGO_TERM_QUIET", "true");
    } else {
        sh.set_var("CARGO_TERM_VERBOSE", "true");
        sh.set_var("CARGO_TERM_QUIET", "false");
    }
}

/// Change to the repository root directory.
///
/// # Panics
///
/// Panics if not in a git repository or git command fails.
pub fn change_to_repo_root(sh: &Shell) {
    let repo_dir = quiet_cmd!(sh, "git rev-parse --show-toplevel")
        .read()
        .expect("Failed to get repository root, ensure you're in a git repository");
    sh.change_dir(&repo_dir);
}

/// Get list of package names and their directories in the workspace using cargo metadata.
/// Returns tuples of (`package_name`, `directory_path`) to support various workspace layouts including nested crates.
///
/// # Arguments
///
/// * `packages` - Optional filter for specific package names. If empty, returns all packages.
///
/// # Errors
///
/// Returns an error if any requested package name doesn't exist in the workspace.
pub fn get_packages(
    sh: &Shell,
    packages: &[String],
) -> Result<Vec<(String, PathBuf)>, Box<dyn std::error::Error>> {
    let metadata = quiet_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let all_packages: Vec<(String, PathBuf)> = json["packages"]
        .as_array()
        .ok_or("Missing 'packages' field in cargo metadata")?
        .iter()
        .filter_map(|package| {
            let package_name = package["name"].as_str()?;
            let manifest_path = package["manifest_path"].as_str()?;
            // Extract directory path from the manifest path,
            // e.g., "/path/to/repo/releases/Cargo.toml" -> "/path/to/repo/releases".
            let dir_path = manifest_path.trim_end_matches("/Cargo.toml");

            Some((package_name.to_owned(), PathBuf::from(dir_path)))
        })
        .collect();

    // If no package filter specified, return all packages.
    if packages.is_empty() {
        return Ok(all_packages);
    }

    // Resolve each requested string to a canonical manifest name,
    // falling back to directory basename matching if no manifest name matches.
    let mut resolved_names: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for requested in packages {
        // Exact manifest name match.
        if all_packages.iter().any(|(name, _)| name == requested) {
            resolved_names.push(requested.clone());
            continue;
        }

        // Fall back to directory basename match.
        let dir_matches: Vec<&(String, PathBuf)> = all_packages
            .iter()
            .filter(|(_, dir)| {
                dir.file_name().and_then(|n| n.to_str()).is_some_and(|n| n == requested)
            })
            .collect();

        match dir_matches.len() {
            0 => {
                errors.push(format!("Package not found in workspace: '{}'", requested));
            }
            1 => {
                let (name, _) = dir_matches[0];
                resolved_names.push(name.clone());
            }
            _ => {
                errors.push(format!(
                    "Ambiguous package '{}': use the manifest name to disambiguate.",
                    requested
                ));
            }
        }
    }

    if !errors.is_empty() {
        let mut error_msg = errors.join("\n\n");

        error_msg.push_str("\n\nAvailable packages (manifest name / directory):");
        for (name, dir) in &all_packages {
            error_msg.push_str(&format!("\n  - {} ({})", name, dir.display()));
        }

        return Err(error_msg.into());
    }

    // Filter to only resolved packages.
    let package_info: Vec<(String, PathBuf)> = all_packages
        .into_iter()
        .filter(|(name, _)| resolved_names.iter().any(|r| r == name))
        .collect();

    Ok(package_info)
}

/// Get the cargo target directory from metadata.
///
/// This respects `CARGO_TARGET_DIR`, .cargo/config.toml, and other cargo
/// target directory configuration.
pub fn get_target_dir(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let metadata = quiet_cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let target_dir =
        json["target_directory"].as_str().ok_or("Missing target_directory in cargo metadata")?;

    Ok(target_dir.to_string())
}
