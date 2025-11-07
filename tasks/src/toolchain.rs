use std::path::Path;
use xshell::{cmd, Shell};

/// Toolchain requirement for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let current = cmd!(sh, "rustc --version").read()?;

    match required {
        Toolchain::Nightly => {
            if !current.contains("nightly") {
                return Err(format!("Need a nightly compiler; have {}", current).into());
            }
        }
        Toolchain::Stable => {
            if current.contains("nightly") || current.contains("beta") {
                return Err(format!("Need a stable compiler; have {}", current).into());
            }
        }
        Toolchain::Msrv => {
            let manifest_path = sh.current_dir().join("Cargo.toml");

            if !manifest_path.exists() {
                return Err("Not in a crate directory (no Cargo.toml found)".into());
            }

            let msrv_version = get_msrv_from_manifest(&manifest_path)?;
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

/// Extract MSRV from Cargo.toml using cargo metadata.
fn get_msrv_from_manifest(manifest_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let sh = Shell::new()?;
    let metadata = cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let data: serde_json::Value = serde_json::from_str(&metadata)?;

    // Convert Path to string for comparison. If path contains invalid UTF-8, fail early.
    let manifest_path_str = manifest_path.to_str().ok_or_else(|| {
        format!(
            "Manifest path contains invalid UTF-8: {}",
            manifest_path.display()
        )
    })?;

    let msrv = data["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|pkg| pkg["manifest_path"].as_str() == Some(manifest_path_str))
        })
        .and_then(|pkg| pkg["rust_version"].as_str())
        .ok_or_else(|| {
            format!(
                "No MSRV (rust-version) specified in {}",
                manifest_path.display()
            )
        })?;

    Ok(msrv.to_string())
}

/// Extract version number from rustc --version output.
/// Example: "rustc 1.74.0 (79e9716c9 2023-11-13)" -> Some("1.74.0")
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
        assert_eq!(
            extract_version("rustc 1.74.0 (79e9716c9 2023-11-13)"),
            Some("1.74.0")
        );
        assert_eq!(
            extract_version("rustc 1.75.0-nightly (12345abcd 2023-11-20)"),
            Some("1.75.0")
        );
        assert_eq!(extract_version("rustc 1.74.0"), Some("1.74.0"));
        assert_eq!(extract_version("rustc unknown version"), None);
        assert_eq!(extract_version("no version here"), None);
    }
}
