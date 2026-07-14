// SPDX-License-Identifier: MIT AND Apache-2.0

//! Run arbitrary cargo commands with specified toolchain and lockfile management.

use xshell::Shell;

use crate::environment::{cargo_cmd, get_workspace_packages, CmdExt, ProgressGuard};
use crate::git;
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run a cargo command with the specified toolchain and lockfile.
///
/// # Arguments
///
/// * `sh` - The shell environment.
/// * `lockfile` - Which lockfile variant to use (minimal, recent, or existing).
/// * `toolchain` - Which toolchain to use (nightly, stable, or msrv).
/// * `baseline` - Optional baseline ref for running the command on multiple commits.
/// * `packages` - Packages to run the command on (empty = all packages).
/// * `cargo_args` - Arguments to pass to cargo (everything after `--`).
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    toolchain: Toolchain,
    baseline: Option<&str>,
    packages: &[String],
    cargo_args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();

    git::for_each_commit(sh, lockfile, baseline, |sh| {
        // Set toolchain and packages per-commit.
        prepare_toolchain(sh, toolchain)?;
        let resolved_packages = get_workspace_packages(sh, packages)?;

        let mut cmd = cargo_cmd(sh);
        // Add cargo subcommand (first arg in cargo_args).
        if let Some(subcommand) = cargo_args.first() {
            cmd = cmd.arg(subcommand);
        }
        // Add package flags after subcommand, but before other arguments.
        for pkg in &resolved_packages {
            cmd = cmd.arg("-p").arg(&pkg.id);
        }
        // Add remaining arguments (skip first which was the subcommand).
        if cargo_args.len() > 1 {
            cmd = cmd.args(&cargo_args[1..]);
        }

        cmd.run_with_capture()
    })?;

    Ok(())
}
