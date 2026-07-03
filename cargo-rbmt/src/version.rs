// SPDX-License-Identifier: MIT AND Apache-2.0

//! Runtime version checking for cargo-rbmt.
//!
//! This module reads `rbmt.version` from the workspace/package Cargo.toml and validates
//! that the running cargo-rbmt version matches any pinned version requirement.

use std::fs;

use serde::Deserialize;
use xshell::Shell;

use crate::environment::{get_workspace_root, WorkspaceManifest};

/// RBMT configuration read from Cargo.toml.
#[derive(Deserialize, Default)]
struct RbmtConfig {
    /// The expected version of cargo-rbmt for this workspace/package.
    version: Option<String>,
}

/// Check if the workspace has a version requirement and validate it against the running
/// `cargo-rbmt` version.
///
/// This reads `rbmt.version` from `[workspace.metadata]` or `[package.metadata]` and validates
/// that the running cargo-rbmt version matches any pinned version requirement. Users can pin to
/// either a semantic version (`rbmt.version = "0.4.1"`) a full git commit hash (`rbmt.version =
/// "abc123def..."`).
///
/// If no version requirement is specified, the check is skipped.
pub fn check(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = get_workspace_root(sh)?.join("Cargo.toml");
    let contents = fs::read_to_string(manifest_path)?;
    let toml: WorkspaceManifest<RbmtConfig> = toml::from_str(&contents)?;

    // Check workspace namespace first, fallback to package namespace.
    let expected_version =
        toml.workspace.metadata.rbmt.version.or(toml.package.metadata.rbmt.version);

    if let Some(expected) = expected_version {
        let actual_version = env!("CARGO_PKG_VERSION");
        let actual_hash = env!("RBMT_GIT_HASH");

        // If expected looks like a git hash (40+ hex chars), compare against commit hash,
        // otherwise compare as a semantic version string.
        if expected.len() >= 40 && expected.chars().all(|c| c.is_ascii_hexdigit()) {
            // If actual_hash is empty (e.g., installed from crates.io registry),
            // we cannot validate against a git hash requirement.
            if actual_hash.is_empty() {
                return Err(format!(
                    "cargo-rbmt version mismatch: expected commit {}, but git hash unavailable (likely installed from registry)",
                    expected
                )
                .into());
            }
            if !actual_hash.starts_with(&expected) {
                return Err(format!(
                    "cargo-rbmt version mismatch: expected commit {}, found {}",
                    expected, actual_hash
                )
                .into());
            }
        } else {
            if expected != actual_version {
                return Err(format!(
                    "cargo-rbmt version mismatch: expected {}, found {}",
                    expected, actual_version
                )
                .into());
            }
        }
    }

    Ok(())
}
