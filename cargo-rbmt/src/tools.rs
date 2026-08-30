// SPDX-License-Identifier: MIT AND Apache-2.0

//! Management of pinned external cargo tools and the cargo-rbmt version itself.
//!
//! Cargo currently has no native mechanism for pinning the versions of tools
//! installed via `cargo install`. The `Cargo.lock` file only covers
//! dependencies of packages in the current workspace, not standalone binaries.
//!
//! ## Configuration
//!
//! Tool versions are stored in the root `Cargo.toml`. The preferred location is
//! `[workspace.metadata.rbmt.tools]`, which works for multi-crate workspaces
//! and single-package repos with an explicit `[workspace]` table.
//!
//! ```toml
//! [workspace.metadata.rbmt.tools]
//! cargo-semver-checks = "0.46.0"
//! ```
//!
//! For single-package repos with no explicit `[workspace]` table,
//! `[package.metadata.rbmt.tools]` is used as a fallback.
//!
//! ## cargo-rbmt version pin
//!
//! In addition to external tools, this module also manages the pinned version
//! of cargo-rbmt itself via `[workspace.metadata.rbmt.version]` (or the
//! package-level fallback). When no filter is given, or when `cargo-rbmt` is
//! explicitly named in the filter, the `tools` subcommand will install or
//! update cargo-rbmt alongside the external tools.
//!
//! ```toml
//! [workspace.metadata]
//! rbmt.version = "0.5.3"
//! rbmt.tools = { zizmor = "1.23.1" }
//! ```

use std::collections::BTreeMap;

use xshell::Shell;

use crate::environment::{get_workspace_root, CmdExt, ProgressGuard, WorkspaceManifest};

/// Filter name to include the cargo-rbmt version pin in a `tools` operation.
pub const RBMT_NAME: &str = "cargo-rbmt";

/// Where the tool pins were found in the root `Cargo.toml`.
///
/// `[workspace.metadata.rbmt.tools]` is preferred and works for both
/// multi-crate workspaces and single-package repos that have an explicit
/// `[workspace]` table. `[package.metadata.rbmt.tools]` is the fallback for
/// single-package repos with no explicit `[workspace]` table.
#[derive(Default)]
enum ToolsLocation {
    #[default]
    Workspace,
    Package,
}

/// The pinned tool versions, where they were found, and the rbmt version pin.
///
/// Defaults to an empty tools map, `Workspace` location, and `None` rbmt
/// version — suitable for bootstrapping when nothing is configured yet.
#[derive(Default)]
struct Tools {
    map: BTreeMap<String, String>,
    location: ToolsLocation,
    /// The pinned cargo-rbmt version if set.
    rbmt_version: Option<String>,
}

impl Tools {
    /// Returns the TOML key path for error messages.
    fn table_name(&self) -> &'static str {
        match self.location {
            ToolsLocation::Workspace => "[workspace.metadata.rbmt.tools]",
            ToolsLocation::Package => "[package.metadata.rbmt.tools]",
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct RbmtTable {
    tools: Option<BTreeMap<String, String>>,
    version: Option<String>,
}

/// Read tool pins and the rbmt version pin from the root `Cargo.toml`.
///
/// Tries `[workspace.metadata.rbmt]` first, then falls back to
/// `[package.metadata.rbmt]`. Returns default-empty `Tools` if neither is
/// present, so callers can still bootstrap an `rbmt.version` pin.
fn read_tools(sh: &Shell) -> Result<Tools, Box<dyn std::error::Error>> {
    let root = get_workspace_root(sh)?;
    let contents = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let cargo_toml = toml::from_str::<WorkspaceManifest<RbmtTable>>(&contents)?;

    if let Some(map) = cargo_toml.workspace.metadata.rbmt.tools {
        return Ok(Tools {
            map,
            location: ToolsLocation::Workspace,
            rbmt_version: cargo_toml.workspace.metadata.rbmt.version,
        });
    }
    if cargo_toml.workspace.metadata.rbmt.version.is_some() {
        return Ok(Tools {
            map: BTreeMap::new(),
            location: ToolsLocation::Workspace,
            rbmt_version: cargo_toml.workspace.metadata.rbmt.version,
        });
    }

    if let Some(map) = cargo_toml.package.metadata.rbmt.tools {
        return Ok(Tools {
            map,
            location: ToolsLocation::Package,
            rbmt_version: cargo_toml.package.metadata.rbmt.version,
        });
    }
    if cargo_toml.package.metadata.rbmt.version.is_some() {
        return Ok(Tools {
            map: BTreeMap::new(),
            location: ToolsLocation::Package,
            rbmt_version: cargo_toml.package.metadata.rbmt.version,
        });
    }

    Ok(Tools::default())
}

/// Write an updated version for a single tool into the appropriate metadata table.
fn write_tool_version(
    sh: &Shell,
    name: &str,
    version: &str,
    location: &ToolsLocation,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = get_workspace_root(sh)?;
    let path = root.join("Cargo.toml");
    let contents = std::fs::read_to_string(&path)?;

    let mut doc: toml_edit::DocumentMut = contents.parse()?;
    let table = match location {
        ToolsLocation::Workspace => &mut doc["workspace"]["metadata"]["rbmt"]["tools"],
        ToolsLocation::Package => &mut doc["package"]["metadata"]["rbmt"]["tools"],
    };
    table[name] = toml_edit::value(version);
    std::fs::write(&path, doc.to_string())?;

    Ok(())
}

/// Write the pinned cargo-rbmt version to the appropriate metadata table.
fn write_rbmt_version(
    sh: &Shell,
    version: &str,
    location: &ToolsLocation,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = get_workspace_root(sh)?;
    let path = root.join("Cargo.toml");
    let contents = std::fs::read_to_string(&path)?;

    let mut doc: toml_edit::DocumentMut = contents.parse()?;
    let table = match location {
        ToolsLocation::Workspace => &mut doc["workspace"]["metadata"]["rbmt"],
        ToolsLocation::Package => &mut doc["package"]["metadata"]["rbmt"],
    };
    table["version"] = toml_edit::value(version);
    std::fs::write(&path, doc.to_string())?;

    Ok(())
}

/// Read the installed version of a crate from `cargo install --list` output.
///
/// ```text
/// crate-name v1.2.3:
///     binary-name
/// ```
///
/// Returns `None` if the crate is not currently installed.
fn installed_version(
    sh: &Shell,
    crate_name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let output = rbmt_cmd!(sh, "cargo install --list").read()?;

    let prefix = format!("{} v", crate_name);
    let version = output
        .lines()
        .find(|line| line.starts_with(&prefix))
        .and_then(|line| line.strip_prefix(&prefix))
        .and_then(|rest| rest.split([' ', ':']).next())
        .map(str::to_string);

    Ok(version)
}

/// Install a single tool at a pinned version using `cargo install`.
fn install_tool(sh: &Shell, name: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Installing {}@{}", name, version);
    rbmt_cmd!(sh, "cargo install {name} --version {version} --locked").run_with_capture()?;
    Ok(())
}

/// Install a single tool at the latest version and return the resolved version.
fn install_tool_latest(sh: &Shell, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    rbmt_eprintln!("Installing {} (latest)", name);
    rbmt_cmd!(sh, "cargo install {name} --locked").run_with_capture()?;

    installed_version(sh, name)?
        .ok_or_else(|| format!("{} not found in `cargo install --list` after install", name).into())
}

/// Install each tool at its pinned version.
fn install_tools(
    sh: &Shell,
    tools: &BTreeMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in tools {
        install_tool(sh, name, version)?;
    }
    Ok(())
}

/// Install each tool at its latest version and update the pins.
fn update_tools(
    sh: &Shell,
    tools: &BTreeMap<String, String>,
    location: &ToolsLocation,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, pinned_version) in tools {
        let latest = install_tool_latest(sh, name)?;
        if &latest == pinned_version {
            rbmt_eprintln!("{} is already at latest ({})", name, pinned_version);
        } else {
            rbmt_eprintln!("Updated {} {} -> {}", name, pinned_version, latest);
            write_tool_version(sh, name, &latest, location)?;
        }
    }
    Ok(())
}

/// Parse the filter list into a filtered tools map and an `include_rbmt` flag.
///
/// `cargo-rbmt` is stripped from the filter and returned as `include_rbmt`.
/// When the filter is empty before stripping, `include_rbmt` is true
/// (defaults to "everything"). Unknown tool names are errors.
fn select(
    filter: &[String],
    tools: &Tools,
) -> Result<(BTreeMap<String, String>, bool), Box<dyn std::error::Error>> {
    let include_rbmt = filter.is_empty() || filter.iter().any(|n| n == RBMT_NAME);
    let tool_names: Vec<&String> = filter.iter().filter(|n| n.as_str() != RBMT_NAME).collect();

    for name in &tool_names {
        if !tools.map.contains_key(*name) {
            return Err(format!("'{}' is not in {}", name, tools.table_name()).into());
        }
    }

    let filtered = if tool_names.is_empty() {
        tools.map.clone()
    } else {
        tools
            .map
            .iter()
            .filter(|(name, _)| tool_names.contains(name))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    Ok((filtered, include_rbmt))
}

/// Install and manage pinned tools and the cargo-rbmt version itself.
///
/// When `update` is false, each tool and cargo-rbmt are installed at their
/// pinned versions. When `update` is true, each tool is installed at its latest
/// version and the pins are updated in place, including the cargo-rbmt version
/// pin.
///
/// The filter controls which items are operated on. When empty, everything is
/// included (all external tools plus cargo-rbmt). The special name
/// `cargo-rbmt` includes the rbmt version pin in the operation.
pub fn run(sh: &Shell, update: bool, filter: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();
    rbmt_eprintln!("Installing tools...");

    let tools = read_tools(sh)?;
    let (selected_tools, include_rbmt) = select(filter, &tools)?;

    if update {
        update_tools(sh, &selected_tools, &tools.location)?;
        if include_rbmt {
            if let Some(ref pinned) = tools.rbmt_version {
                let latest = install_tool_latest(sh, RBMT_NAME)?;
                if pinned != &latest {
                    write_rbmt_version(sh, &latest, &tools.location)?;
                }
            }
        }
    } else {
        install_tools(sh, &selected_tools)?;
        if include_rbmt {
            if let Some(ref pin) = tools.rbmt_version {
                install_tool(sh, RBMT_NAME, pin)?;
            }
        }
    }

    Ok(())
}
