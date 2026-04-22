//! Rust toolchain management.
//!
//! Toolchain versions are stored in the root `Cargo.toml` manifest. The preferred location is
//! `[workspace.metadata.rbmt.toolchains]`, which works for multi-crate workspaces
//! and single-package repos with an explicit `[workspace]` table.
//!
//! ```toml
//! [workspace.metadata.rbmt.toolchains]
//! nightly = "nightly-2026-02-21"
//! stable = "1.94.0"
//! ```
//!
//! For single-package repos with no explicit `[workspace]` table,
//! `[package.metadata.rbmt.toolchains]` is used as a fallback.

use std::fs;
use std::path::Path;

use toml_edit::DocumentMut;
use xshell::Shell;

use crate::environment::{get_workspace_root, WorkspaceManifest};

/// Where the toolchain pins were found in the root `Cargo.toml`.
///
/// `[workspace.metadata.rbmt.toolchains]` is preferred and works for both
/// multi-crate workspaces and single-package repos that have an explicit
/// `[workspace]` table. `[package.metadata.rbmt.toolchains]` is the fallback for
/// single-package repos with no explicit `[workspace]` table.
#[derive(Debug)]
enum ToolchainsLocation {
    Workspace,
    Package,
}

impl ToolchainsLocation {
    /// Returns the TOML key path for error messages.
    fn table_name(&self) -> &'static str {
        match self {
            Self::Workspace => "[workspace.metadata.rbmt.toolchains]",
            Self::Package => "[package.metadata.rbmt.toolchains]",
        }
    }
}

/// The pinned toolchain versions and where they were found.
struct ToolchainsConfigData {
    nightly: Option<String>,
    stable: Option<String>,
    location: ToolchainsLocation,
}

/// Deserializes the `[*.metadata.rbmt]` table.
#[derive(serde::Deserialize, Default)]
struct RbmtTable {
    #[serde(default)]
    toolchains: Option<ToolchainsConfig>,
}

/// The `[workspace.metadata.rbmt.toolchains]` or `[package.metadata.rbmt.toolchains]` table,
/// holding pinned toolchain versions.
#[derive(serde::Deserialize)]
struct ToolchainsConfig {
    /// Pinned nightly toolchain version, e.g. `"nightly-2025-02-17"`.
    nightly: Option<String>,
    /// Pinned stable toolchain version, e.g. `"1.85.0"`.
    stable: Option<String>,
}

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
    /// Reads from either `[workspace.metadata.rbmt.toolchains]` or
    /// `[package.metadata.rbmt.toolchains]` (with workspace taking precedence).
    pub fn read_version(self, sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
        let config = Self::read_toolchains_config(sh)?;

        match self {
            Self::Nightly => config.nightly.ok_or_else(|| {
                format!("No pinned nightly toolchain found in {}", config.location.table_name())
                    .into()
            }),
            Self::Stable => config.stable.ok_or_else(|| {
                format!("No pinned stable toolchain found in {}", config.location.table_name())
                    .into()
            }),
            Self::Msrv => get_workspace_msrv(sh),
        }
    }

    /// Write an updated version to Cargo.toml.
    ///
    /// Writes to the same location where the toolchains were originally found
    /// (either `[workspace.metadata.rbmt.toolchains]` or `[package.metadata.rbmt.toolchains]`).
    ///
    /// # Errors
    ///
    /// Returns an error if trying to write MSRV which is derived from Cargo.toml, not editable.
    pub fn write_version(
        self,
        sh: &Shell,
        version: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = get_workspace_root(sh)?;
        let path = root.join("Cargo.toml");
        let contents = std::fs::read_to_string(&path)?;
        let mut doc: toml_edit::DocumentMut = contents.parse()?;

        let table = match Self::read_toolchains_config(sh)?.location {
            ToolchainsLocation::Workspace =>
                &mut doc["workspace"]["metadata"]["rbmt"]["toolchains"],
            ToolchainsLocation::Package => &mut doc["package"]["metadata"]["rbmt"]["toolchains"],
        };

        match self {
            Self::Nightly => {
                table["nightly"] = toml_edit::value(version);
            }
            Self::Stable => {
                table["stable"] = toml_edit::value(version);
            }
            Self::Msrv =>
                return Err(
                    "Cannot update MSRV via write_version; it's derived from Cargo.toml".into()
                ),
        }

        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }

    /// Read toolchain pins from the root `Cargo.toml`.
    ///
    /// Tries `[workspace.metadata.rbmt.toolchains]` first, then falls back to
    /// `[package.metadata.rbmt.toolchains]`. Returns an error if neither is present.
    fn read_toolchains_config(
        sh: &Shell,
    ) -> Result<ToolchainsConfigData, Box<dyn std::error::Error>> {
        let root = get_workspace_root(sh)?;
        let contents = std::fs::read_to_string(root.join("Cargo.toml"))?;
        let cargo_toml = toml::from_str::<WorkspaceManifest<RbmtTable>>(&contents)?;

        // Try workspace first.
        if let Some(toolchains) = cargo_toml.workspace.metadata.rbmt.toolchains {
            return Ok(ToolchainsConfigData {
                nightly: toolchains.nightly,
                stable: toolchains.stable,
                location: ToolchainsLocation::Workspace,
            });
        }

        // Fall back to package.
        if let Some(toolchains) = cargo_toml.package.metadata.rbmt.toolchains {
            return Ok(ToolchainsConfigData {
                nightly: toolchains.nightly,
                stable: toolchains.stable,
                location: ToolchainsLocation::Package,
            });
        }

        Err("No [workspace.metadata.rbmt.toolchains] or [package.metadata.rbmt.toolchains] exists."
            .into())
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
    let current = rbmt_cmd!(sh, "rustc --version").read()?;

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
/// Reads the pinned toolchain from `[workspace.metadata.rbmt.toolchains]` or
/// `[package.metadata.rbmt.toolchains]` and sets it via `sh.set_var`, which only
/// affects child processes spawned through this shell instance and does not mutate
/// the process environment seen by `std::env::var`.
///
/// If the caller passed `+toolchain` (e.g. `cargo +nightly rbmt lint`), rustup already set
/// `RUSTUP_TOOLCHAIN` in the process environment before this binary ran. We deliberately
/// overwrite it with the pinned version because `Cargo.toml` is the authoritative source of
/// truth for which toolchain each task requires. Falls back silently when rustup is not
/// available (e.g. Nix) or when no version is configured.
fn maybe_set_rustup_toolchain(sh: &Shell, required: Toolchain) {
    // Only attempt if rustup is available.
    if rbmt_cmd!(sh, "rustup --version").read().is_err() {
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

/// Parse rust-version directly from a Cargo.toml file.
///
/// Used as a fallback when cargo metadata doesn't include the `rust_version` field
/// which happens with older rust versions.
fn parse_rust_version_from_toml(
    manifest_path: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(manifest_path)?;
    let doc = content.parse::<DocumentMut>()?;

    if let Some(package) = doc.get("package").and_then(|p| p.as_table()) {
        if let Some(rust_version) = package.get("rust-version") {
            if let Some(version_str) = rust_version.as_str() {
                return Ok(Some(version_str.to_string()));
            }
        }
    }

    Ok(None)
}

/// Collect all MSRVs in the workspace.
fn collect_msrvs(sh: &Shell) -> Result<Vec<ManifestMsrv>, Box<dyn std::error::Error>> {
    let metadata = rbmt_cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let data: serde_json::Value = serde_json::from_str(&metadata)?;

    Ok(data["packages"]
        .as_array()
        .map(|packages| {
            packages
                .iter()
                .filter_map(|pkg| {
                    let manifest_path = pkg["manifest_path"].as_str()?.to_string();

                    // Try to get rust_version from metadata first,
                    // then attempt direct parsing of manifest toml
                    // as a fallback.
                    let rust_version = pkg["rust_version"]
                        .as_str()
                        .map(str::to_string)
                        .or_else(|| parse_rust_version_from_toml(&manifest_path).ok().flatten());

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
