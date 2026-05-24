// SPDX-License-Identifier: MIT AND Apache-2.0

//! Benchmark testing tasks.

use xshell::Shell;

use crate::environment::{cargo_cmd, get_workspace_packages, CmdExt, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run benchmark tests for all crates in the workspace.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;
    rbmt_eprintln!("Running bench tests for {} crates", packages.len());

    for package in packages {
        rbmt_eprintln!("Running bench tests in: {}", package.dir.display());
        let _dir_guard = sh.push_dir(&package.dir);
        cargo_cmd(sh).arg("bench").run_verbose()?;
    }

    rbmt_eprintln!("Benches complete.");
    Ok(())
}
