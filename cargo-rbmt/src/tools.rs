//! Management of pinned external cargo tools.
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

use std::collections::BTreeMap;

use xshell::Shell;

use crate::environment::{get_workspace_root, ProgressGuard, WorkspaceManifest};

/// Where the tool pins were found in the root `Cargo.toml`.
///
/// `[workspace.metadata.rbmt.tools]` is preferred and works for both
/// multi-crate workspaces and single-package repos that have an explicit
/// `[workspace]` table. `[package.metadata.rbmt.tools]` is the fallback for
/// single-package repos with no explicit `[workspace]` table.
enum ToolsLocation {
    Workspace,
    Package,
}

/// The pinned tool versions and where they were found.
struct Tools {
    map: BTreeMap<String, String>,
    location: ToolsLocation,
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
}

/// Read tool pins from the root `Cargo.toml`.
///
/// Tries `[workspace.metadata.rbmt.tools]` first, then falls back to
/// `[package.metadata.rbmt.tools]`. Returns `None` if neither table is present.
fn read_tools(sh: &Shell) -> Result<Option<Tools>, Box<dyn std::error::Error>> {
    let root = get_workspace_root(sh)?;
    let contents = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let cargo_toml = toml::from_str::<WorkspaceManifest<RbmtTable>>(&contents)?;

    if let Some(map) = cargo_toml.workspace.metadata.rbmt.tools {
        return Ok(Some(Tools { map, location: ToolsLocation::Workspace }));
    }

    if let Some(map) = cargo_toml.package.metadata.rbmt.tools {
        return Ok(Some(Tools { map, location: ToolsLocation::Package }));
    }

    Ok(None)
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
    rbmt_cmd!(sh, "cargo install {name} --version {version} --locked").run()?;
    Ok(())
}

/// Install a single tool at the latest version and return the resolved version.
fn install_tool_latest(sh: &Shell, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    rbmt_eprintln!("Installing {} (latest)", name);
    rbmt_cmd!(sh, "cargo install {name}").run()?;

    installed_version(sh, name)?
        .ok_or_else(|| format!("{} not found in `cargo install --list` after install", name).into())
}

/// Install all tools pinned in the root `Cargo.toml`.
///
/// When `update` is true, each tool is installed at its latest version and the
/// pin in `Cargo.toml` is updated in place to match. When false, each tool is
/// installed at its pinned version.
///
/// When `filter` is non-empty, only the named tools are operated on. Unknown
/// tool names in the filter are treated as an error.
pub fn run(sh: &Shell, update: bool, filter: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();
    rbmt_eprintln!("Installing tools...");
    let Some(mut tools) = read_tools(sh)? else {
        rbmt_eprintln!(
            "No tools found in [workspace.metadata.rbmt.tools] or [package.metadata.rbmt.tools]."
        );
        return Ok(());
    };

    if !filter.is_empty() {
        for name in filter {
            if !tools.map.contains_key(name) {
                return Err(format!("'{}' is not in {}", name, tools.table_name()).into());
            }
        }
        tools.map.retain(|name, _| filter.contains(name));
    }

    for (name, pinned_version) in &tools.map {
        if update {
            let latest = install_tool_latest(sh, name)?;
            if &latest == pinned_version {
                rbmt_eprintln!("{} is already at latest ({})", name, pinned_version);
            } else {
                rbmt_eprintln!("Updated {} {} -> {}", name, pinned_version, latest);
                write_tool_version(sh, name, &latest, &tools.location)?;
            }
        } else {
            install_tool(sh, name, pinned_version)?;
        }
    }

    Ok(())
}
