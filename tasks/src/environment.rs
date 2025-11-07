use std::env;
use xshell::{cmd, Shell};

/// Environment variable to control output verbosity.
/// Set to "quiet" to suppress informational messages and reduce cargo output.
/// Any other value (or unset) defaults to verbose mode.
const LOG_LEVEL_ENV_VAR: &str = "RBMT_LOG_LEVEL";

/// Path to the RBMT configuration file relative to workspace/crate root.
pub const CONFIG_FILE_PATH: &str = "contrib/rbmt.toml";

/// Check if we're in quiet mode via environment variable.
pub fn is_quiet_mode() -> bool {
    env::var(LOG_LEVEL_ENV_VAR).is_ok_and(|v| v == "quiet")
}

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

/// Print a message unless in quiet mode.
pub fn quiet_println(msg: &str) {
    if !is_quiet_mode() {
        println!("{}", msg);
    }
}

/// Configure shell log level and output verbosity.
/// Sets cargo output verbosity based on LOG_LEVEL_ENV_VAR.
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
    let repo_dir = cmd!(sh, "git rev-parse --show-toplevel")
        .read()
        .expect("Failed to get repository root, ensure you're in a git repository");
    sh.change_dir(&repo_dir);
}

/// Get list of crate directories in the workspace using cargo metadata.
/// Returns fully qualified paths to support various workspace layouts including nested crates.
pub fn get_crate_dirs(sh: &Shell) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let metadata = cmd!(sh, "cargo metadata --no-deps --format-version 1").read()?;
    let json: serde_json::Value = serde_json::from_str(&metadata)?;

    let crate_dirs: Vec<String> = json["packages"]
        .as_array()
        .ok_or("Missing 'packages' field in cargo metadata")?
        .iter()
        .filter_map(|package| {
            let manifest_path = package["manifest_path"].as_str()?;
            // Extract directory path from the manifest path,
            // e.g., "/path/to/repo/releases/Cargo.toml" -> "/path/to/repo/releases".
            let dir_path = manifest_path.trim_end_matches("/Cargo.toml");
            Some(dir_path.to_string())
        })
        .collect();

    Ok(crate_dirs)
}
