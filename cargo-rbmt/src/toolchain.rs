use std::path::Path;

use xshell::Shell;

use crate::quiet_cmd;

/// Environment variable that rustup's shims read to route `rustc`, `cargo`, and
/// other toolchain commands to a specific installed toolchain.
///
/// When a user invokes `cargo +nightly <subcommand>`, Cargo translates the
/// `+nightly` flag into `RUSTUP_TOOLCHAIN=nightly` before exec-ing the rustup
/// shim, so all child processes spawned during that invocation inherit the
/// correct toolchain automatically. Setting this variable directly has the same
/// effect and is the supported mechanism for propagating a toolchain choice into
/// subprocesses without repeating the `+toolchain` flag on every inner call.
const RUSTUP_TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";

/// Version file that pins the nightly toolchain.
const NIGHTLY_VERSION_FILE: &str = "nightly-version";

/// Version file that pins the stable toolchain.
const STABLE_VERSION_FILE: &str = "stable-version";

/// Toolchain requirement for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Toolchain {
    /// Nightly toolchain.
    Nightly,
    /// Stable toolchain.
    Stable,
    /// Minimum Supported Rust Version.
    Msrv,
}

impl Toolchain {
    /// Read the pinned version for this toolchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the version file is missing or empty, or if the
    /// workspace MSRV cannot be determined.
    pub fn read_version(self, sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            Self::Nightly => Self::read_version_file(sh, NIGHTLY_VERSION_FILE).ok_or_else(|| {
                format!("{} file not found in repository root", NIGHTLY_VERSION_FILE).into()
            }),
            Self::Stable => Self::read_version_file(sh, STABLE_VERSION_FILE).ok_or_else(|| {
                format!("{} file not found in repository root", STABLE_VERSION_FILE).into()
            }),
            Self::Msrv => get_workspace_msrv(sh),
        }
    }

    /// Write a pinned version string for this toolchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written, or if called on [`Toolchain::Msrv`].
    pub fn write_version(
        self,
        sh: &Shell,
        version: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Nightly => Self::write_version_file(sh, NIGHTLY_VERSION_FILE, version),
            Self::Stable => Self::write_version_file(sh, STABLE_VERSION_FILE, version),
            Self::Msrv => Err("MSRV is derived from Cargo.toml and cannot be written".into()),
        }
    }

    /// Read a version file from the shell's current directory, trimming whitespace.
    fn read_version_file(sh: &Shell, filename: &str) -> Option<String> {
        // Uses `sh.current_dir()` rather than a bare relative path because
        // `sh.change_dir()` only updates xshell's internal working directory, not
        // the process working directory used by `std::fs`.
        let path = sh.current_dir().join(filename);
        if path.exists() {
            std::fs::read_to_string(path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        }
    }

    /// Write a version string to a file in the shell's current directory, with a trailing newline.
    fn write_version_file(
        sh: &Shell,
        filename: &str,
        version: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(sh.current_dir().join(filename), format!("{}\n", version))?;
        Ok(())
    }
}

/// Check if the current toolchain matches the requirement of current crate.
///
/// # Errors
///
/// * Cannot determine current toolchain version.
/// * Current toolchain doesn't match requirement.
/// * For MSRV: cannot read rust-version from Cargo.toml.
pub fn check_toolchain(sh: &Shell, required: Toolchain) -> Result<(), Box<dyn std::error::Error>> {
    let current = quiet_cmd!(sh, "rustc --version").read()?;

    match required {
        Toolchain::Nightly =>
            if !current.contains("nightly") {
                return Err(format!("Need a nightly compiler; have {}", current).into());
            },
        Toolchain::Stable =>
            if current.contains("nightly") || current.contains("beta") {
                return Err(format!("Need a stable compiler; have {}", current).into());
            },
        Toolchain::Msrv => {
            let manifest_path = sh.current_dir().join("Cargo.toml");

            if !manifest_path.exists() {
                return Err("Not in a crate directory (no Cargo.toml found)".into());
            }

            let msrv_version = get_msrv_from_manifest(sh, &manifest_path)?;
            let current_version =
                extract_version(&current).ok_or("Could not parse rustc version")?;

            if current_version != msrv_version {
                return Err(format!(
                    "Need Rust {} for MSRV testing in {}; have {}",
                    msrv_version,
                    manifest_path.display(),
                    current_version
                )
                .into());
            }
        }
    }

    Ok(())
}

/// Auto-select via rustup if available, but always verify.
///
/// Combines [`maybe_set_rustup_toolchain`] and [`check_toolchain`] into a single
/// call for the common case where both should always run together.
///
/// # Errors
///
/// Returns an error if the active toolchain does not match `required` after
/// auto-selection. See [`check_toolchain`] for details.
pub fn prepare_toolchain(
    sh: &Shell,
    required: Toolchain,
) -> Result<(), Box<dyn std::error::Error>> {
    maybe_set_rustup_toolchain(sh, required);
    check_toolchain(sh, required)
}

/// Set `RUSTUP_TOOLCHAIN` on the [`Shell`] to the pinned toolchain version if
/// rustup is available.
///
/// Reads the toolchain from the appropriate version file (`nightly-version`,
/// `stable-version`, or `Cargo.toml` `rust-version`) and sets it via
/// `sh.set_var`, which only affects child processes spawned through this shell
/// instance and does not mutate the process environment seen by
/// `std::env::var`.
///
/// If the caller passed `+toolchain` (e.g. `cargo +nightly rbmt lint`), rustup
/// already set `RUSTUP_TOOLCHAIN` in the process environment before this binary
/// ran. We deliberately overwrite it with the pinned version because the version
/// file is the authoritative source of truth for which toolchain each task
/// requires. Falls back silently when rustup is not available (e.g. Nix) or
/// when no version file is present.
fn maybe_set_rustup_toolchain(sh: &Shell, required: Toolchain) {
    // Only attempt if rustup is available.
    if quiet_cmd!(sh, "rustup --version").ignore_stderr().read().is_err() {
        return;
    }

    if let Ok(toolchain) = required.read_version(sh) {
        sh.set_var(RUSTUP_TOOLCHAIN, toolchain);
    }
}

/// Extract the single MSRV shared across the workspace.
///
/// Collects all `rust-version` fields declared by packages in the workspace and
/// requires exactly one distinct value. Workspaces with multiple MSRVs are
/// not supported.
pub fn get_workspace_msrv(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let mut msrvs: Vec<String> =
        collect_msrvs(sh)?.into_iter().filter_map(|(_, rust_version)| rust_version).collect();

    msrvs.sort();
    msrvs.dedup();

    match msrvs.as_slice() {
        [] => Err("No MSRV (rust-version) found in any Cargo.toml in the workspace".into()),
        [msrv] => Ok(msrv.clone()),
        _ => Err(format!("Workspace packages have conflicting MSRVs: {}", msrvs.join(", ")).into()),
    }
}

/// Extract MSRV from a specific Cargo.toml using cargo metadata.
fn get_msrv_from_manifest(
    sh: &Shell,
    manifest_path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    // Convert Path to string for comparison. If path contains invalid UTF-8, fail early.
    let manifest_path_str = manifest_path.to_str().ok_or_else(|| {
        format!("Manifest path contains invalid UTF-8: {}", manifest_path.display())
    })?;

    collect_msrvs(sh)?
        .into_iter()
        .find(|(path, _)| path == manifest_path_str)
        .and_then(|(_, rust_version)| rust_version)
        .ok_or_else(|| {
            format!("No MSRV (rust-version) specified in {}", manifest_path.display()).into()
        })
}

/// `(manifest_path, rust_version)` pair; `rust_version` is `None` when not declared.
type ManifestMsrv = (String, Option<String>);

/// Collect all MSRVs in the workspace.
fn collect_msrvs(sh: &Shell) -> Result<Vec<ManifestMsrv>, Box<dyn std::error::Error>> {
    let metadata = quiet_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let data: serde_json::Value = serde_json::from_str(&metadata)?;

    Ok(data["packages"]
        .as_array()
        .map(|packages| {
            packages
                .iter()
                .filter_map(|pkg| {
                    let manifest_path = pkg["manifest_path"].as_str()?.to_string();
                    let rust_version = pkg["rust_version"].as_str().map(str::to_string);
                    Some((manifest_path, rust_version))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Extract version number from rustc --version output.
///
/// # Examples
///
/// `"rustc 1.74.0 (79e9716c9 2023-11-13)"` -> `Some("1.74.0")`
fn extract_version(rustc_version: &str) -> Option<&str> {
    rustc_version.split_whitespace().find_map(|part| {
        // Split off any suffix like "-nightly" or "-beta".
        let version_part = part.split('-').next()?;

        // Version format: digit.digit.digit
        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
            Some(version_part)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("rustc 1.74.0 (79e9716c9 2023-11-13)"), Some("1.74.0"));
        assert_eq!(extract_version("rustc 1.75.0-nightly (12345abcd 2023-11-20)"), Some("1.75.0"));
        assert_eq!(extract_version("rustc 1.74.0"), Some("1.74.0"));
        assert_eq!(extract_version("rustc unknown version"), None);
        assert_eq!(extract_version("no version here"), None);
    }
}
