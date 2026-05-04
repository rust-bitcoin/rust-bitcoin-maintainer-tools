// SPDX-License-Identifier: MIT AND Apache-2.0

//! Run arbitrary cargo commands with specified toolchain and lockfile management.

use xshell::Shell;

use crate::environment::{cargo_cmd, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run a cargo command with the specified toolchain and lockfile.
///
/// # Arguments
///
/// * `sh` - The shell environment.
/// * `lockfile` - Which lockfile variant to use (minimal, recent, or existing).
/// * `toolchain` - Which toolchain to use (nightly, stable, or msrv).
/// * `cargo_args` - Arguments to pass to cargo (everything after `--`).
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    toolchain: Toolchain,
    cargo_args: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, toolchain)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    rbmt_eprintln!("Running cargo command with {:?} deps and {:?} toolchain", lockfile, toolchain);
    let mut cmd = cargo_cmd(sh);
    for arg in cargo_args {
        cmd = cmd.arg(arg);
    }
    cmd.run()?;
    Ok(())
}
