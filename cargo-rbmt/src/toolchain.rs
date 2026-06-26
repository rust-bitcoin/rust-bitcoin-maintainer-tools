// SPDX-License-Identifier: MIT AND Apache-2.0

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
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;
use xshell::Shell;

use crate::environment::{get_workspace_root, CmdExt, WorkspaceManifest};

/// Fixed components installed on every toolchain.
const COMPONENTS: &str = "rust-src,clippy,rustfmt";
/// Fixed target installed on every toolchain (for no-std cross-compilation testing).
const TARGET: &str = "thumbv7m-none-eabi";

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
    /// Try to read the pinned version for this toolchain, returning `None` if not configured.
    ///
    /// For nightly and stable, returns `None` if not in `[workspace.metadata.rbmt.toolchains]`
    /// or `[package.metadata.rbmt.toolchains]`. For MSRV, returns `None` if no `rust-version`
    /// is found in any workspace package.
    pub fn try_read_version(self, sh: &Shell) -> Option<std::string::String> {
        let config = match Self::read_toolchains_config(sh) {
            Ok(c) => Some(c),
            Err(e) => {
                rbmt_eprintln!("Warning: Could not read toolchains config: {}", e);
                None
            }
        };

        match self {
            Self::Nightly => config.and_then(|c| c.nightly),
            Self::Stable => config.and_then(|c| c.stable),
            Self::Msrv => match get_workspace_msrv(sh) {
                Ok(msrv) => Some(msrv),
                Err(e) => {
                    rbmt_eprintln!("Unable to determine MSRV: {}", e);
                    None
                }
            },
        }
    }

    /// Update the pinned version for this toolchain to the latest available.
    ///
    /// For MSRV, returns an error since MSRV is read-only from Cargo.toml.
    pub fn update_version(self, sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        // Get the existing TOML configuration.
        let root = get_workspace_root(sh)?;
        let path = root.join("Cargo.toml");
        let contents = std::fs::read_to_string(&path)?;
        let mut doc: toml_edit::DocumentMut = contents.parse()?;
        let table = match Self::read_toolchains_config(sh)?.location {
            ToolchainsLocation::Workspace =>
                &mut doc["workspace"]["metadata"]["rbmt"]["toolchains"],
            ToolchainsLocation::Package => &mut doc["package"]["metadata"]["rbmt"]["toolchains"],
        };

        // Fetch latest version and set on config.
        let version = match self {
            Self::Nightly => {
                let v = Self::fetch_latest_nightly()?;
                table["nightly"] = toml_edit::value(&v);
                v
            }
            Self::Stable => {
                let v = Self::fetch_latest_stable()?;
                table["stable"] = toml_edit::value(&v);
                v
            }
            Self::Msrv => return Err("Cannot update MSRV version".into()),
        };

        // Write the new configuration back out.
        std::fs::write(&path, doc.to_string())?;
        rbmt_eprintln!("Updated {:?}: {}", self, version);

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

    /// Fetch the latest nightly version from the rust release channel API.
    fn fetch_latest_nightly() -> Result<String, Box<dyn std::error::Error>> {
        let manifest =
            bitreq::get("https://static.rust-lang.org/dist/channel-rust-nightly.toml").send()?;
        let text = manifest.as_str()?;

        let parsed: toml::Value = toml::from_str(text)?;
        let date = parsed
            .get("date")
            .and_then(|v| v.as_str())
            .ok_or("Could not find 'date' field in nightly channel manifest")?;

        Ok(format!("nightly-{}", date))
    }

    /// Fetch the latest stable version from the rust release channel API.
    fn fetch_latest_stable() -> Result<String, Box<dyn std::error::Error>> {
        let manifest =
            bitreq::get("https://static.rust-lang.org/dist/channel-rust-stable.toml").send()?;
        let text = manifest.as_str()?;

        let parsed: toml::Value = toml::from_str(text)?;
        let rustc_section = parsed
            .get("pkg")
            .and_then(|pkg| pkg.get("rustc"))
            .ok_or("Could not find pkg.rustc section in stable channel manifest")?;

        let version_str = rustc_section
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or("Could not find version field in rustc package")?;

        // Version is like "1.96.0 (ac68faa20 2026-05-25)" - extract just the version number
        let version = version_str
            .split_whitespace()
            .next()
            .ok_or("Could not parse version from rustc package")?;

        Ok(version.to_string())
    }
}

/// Install a single toolchain with the fixed components and target using `rustup`.
pub fn install_toolchain(sh: &Shell, toolchain: &str) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Installing toolchain {}", toolchain);

    // --no-self-update keeps rustup from updating itself, not related to toolchains.
    rbmt_cmd!(
        sh,
        "rustup toolchain install {toolchain} --component {COMPONENTS} --target {TARGET} --no-self-update"
    )
    // An unstable fallback feature which makes updating toolchains more robust
    // when working inside containers (the usual for CI actions). Should not
    // have any effect elsewhere.
    .env("RUSTUP_PERMIT_COPY_RENAME", "true")
    .run_with_capture()?;
    Ok(())
}

/// Reinstall a toolchain by uninstalling and then installing fresh using `rustup`.
/// Used as a fallback when the normal install fails (e.g., due to overlayfs issues in containers).
pub fn reinstall_toolchain(sh: &Shell, toolchain: &str) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Uninstalling toolchain {}", toolchain);
    rbmt_cmd!(sh, "rustup toolchain uninstall {toolchain}").ignore_stdout().run()?;
    install_toolchain(sh, toolchain)
}

/// Ensures a [`Toolchain`] is ready for use (see [`prepare_toolchain_with_override`]).
pub fn prepare_toolchain(
    sh: &Shell,
    required: Toolchain,
) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain_with_override(sh, required, None)
}

/// Ensures a [`Toolchain`] is ready for use.
///
/// Installs the given class of toolchain defined in the manifest if `rustup` is available. For
/// example, if a package defines a given nightly version.
///
/// If `msrv_override` is provided, uses that specific MSRV version instead of what is defined in
/// the manifest configuration. Only valid when `required` is [`Toolchain::Msrv`].
///
/// # Errors
///
/// Returns an error if the active toolchain does not match `required`, if install fails.
pub fn prepare_toolchain_with_override(
    sh: &Shell,
    required: Toolchain,
    msrv_override: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Install the toolchain if we have a version and rustup is available.
    // MSRV override only applies when MSRV is required.
    if let Some(version) = &msrv_override
        .filter(|_| matches!(required, Toolchain::Msrv))
        .map(std::string::ToString::to_string)
        .or_else(|| required.try_read_version(sh))
    {
        if rbmt_cmd!(sh, "rustup --version").ignore_stderr().read().is_ok() {
            if let Err(e) = install_toolchain(sh, version) {
                rbmt_eprintln!("Install failed, retrying with reinstall: {}", e);
                reinstall_toolchain(sh, version)?;
            }
            sh.set_var(RUSTUP_TOOLCHAIN, version.clone());
        }
    }

    // Verify the correct class of toolchain is active.
    let active_toolchain = rbmt_cmd!(sh, "rustc --version").read()?;
    match required {
        Toolchain::Nightly =>
            if !active_toolchain.contains("nightly") {
                return Err(format!("Need a nightly compiler; have {}", active_toolchain).into());
            },
        Toolchain::Stable =>
            if active_toolchain.contains("nightly") || active_toolchain.contains("beta") {
                return Err(format!("Need a stable compiler; have {}", active_toolchain).into());
            },
        Toolchain::Msrv => {
            let active_version =
                extract_version(&active_toolchain).ok_or("Could not parse rustc version")?;

            let msrv_version = if let Some(override_version) = msrv_override {
                rbmt_eprintln!("Using MSRV override: {}", override_version);
                override_version.to_string()
            } else {
                let manifest_path = sh.current_dir().join("Cargo.toml");
                if !manifest_path.exists() {
                    return Err("Not in a crate directory (no Cargo.toml found)".into());
                }
                get_msrv_from_manifest(sh, &manifest_path)?
            };

            if active_version != msrv_version {
                return Err(
                    format!("Need Rust {} but have {}", msrv_version, active_version).into()
                );
            }
        }
    }

    rbmt_eprintln!("The current toolchain is: {}", active_toolchain);
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
    collect_msrvs(sh)?
        .into_iter()
        .find(|(path, _)| path == manifest_path)
        .and_then(|(_, rust_version)| rust_version)
        .ok_or_else(|| {
            format!("No MSRV (rust-version) specified in {}", manifest_path.display()).into()
        })
}

/// `(manifest_path, rust_version)` pair; `rust_version` is `None` when not declared.
type ManifestMsrv = (PathBuf, Option<String>);

/// Parse rust-version directly from a Cargo.toml file.
///
/// Used as a fallback when cargo metadata doesn't include the `rust_version` field
/// which happens with older rust versions.
fn parse_rust_version_from_toml(
    manifest_path: &Path,
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
                    let manifest_path = PathBuf::from(pkg["manifest_path"].as_str()?);

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
