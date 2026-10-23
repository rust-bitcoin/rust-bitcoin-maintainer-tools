// SPDX-License-Identifier: MIT AND Apache-2.0

//! Runtime version checking for cargo-rbmt.
//!
//! This module reads `rbmt.version` from the workspace/package Cargo.toml and validates
//! that the running cargo-rbmt version matches any pinned version requirement. This currently
//! only supports semantic versions, it doesn't play nice with git hashes.

use std::fs;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{get_workspace_root, WorkspaceManifest};

/// Exit code returned when auto-install completes successfully but the requested command was not
/// run. Callers should re-run the command so the newly-installed version executes it.
pub const AUTO_INSTALL_EXIT_CODE: i32 = 79;

/// Outcome of the version check.
pub enum VersionOutcome {
    /// Version matched (or no pin configured).
    Ok,
    /// Auto-install completed successfully.
    Installed,
}

/// RBMT configuration read from Cargo.toml.
#[derive(Deserialize, Default)]
struct RbmtConfig {
    /// The expected version of cargo-rbmt for this workspace/package.
    version: Option<String>,
}

/// Check the pinned version against the running `cargo-rbmt` binary.
///
/// Reads `rbmt.version` from `[workspace.metadata]` (falling back to `[package.metadata]`) and
/// validates it against the running binary.
///
/// When `update` is `false` any mismatch is returned as an error. When `true` the correct version
/// is installed via `cargo install` and [`VersionOutcome::Installed`] is returned.
pub fn check(sh: &Shell, update: bool) -> Result<VersionOutcome, Box<dyn std::error::Error>> {
    let manifest_path = get_workspace_root(sh)?.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest_path)?;
    let toml: WorkspaceManifest<RbmtConfig> = toml::from_str(&contents)?;

    let expected_version =
        toml.workspace.metadata.rbmt.version.or(toml.package.metadata.rbmt.version);

    let Some(expected) = expected_version else {
        rbmt_eprintln!("No rbmt version pin in manifest, continuing");
        return Ok(VersionOutcome::Ok);
    };

    let actual = env!("CARGO_PKG_VERSION");

    if expected != actual {
        rbmt_eprintln!("cargo-rbmt version mismatch!");
        if !update {
            return Err(format!(
                "cargo-rbmt version mismatch: expected {}, found {}",
                expected, actual
            )
            .into());
        }

        crate::tools::install_tool(sh, crate::tools::RBMT_NAME, &expected)?;
        rbmt_eprintln!("Installed cargo-rbmt {}. Re-run your command.", expected);
        return Ok(VersionOutcome::Installed);
    }

    Ok(VersionOutcome::Ok)
}
