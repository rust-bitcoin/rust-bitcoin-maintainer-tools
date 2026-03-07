use std::path::Path;

use xshell::Shell;

use crate::quiet_cmd;

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
