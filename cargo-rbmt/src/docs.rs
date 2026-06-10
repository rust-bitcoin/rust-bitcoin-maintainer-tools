// SPDX-License-Identifier: MIT AND Apache-2.0

//! Documentation building tasks.

use xshell::Shell;

use crate::environment::{cargo_cmd, get_workspace_packages, CmdExt, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Build documentation for end users with the stable toolchain.
///
/// This verifies that `cargo doc` works correctly for users with stable Rust.
/// Uses basic rustdoc warnings to catch common documentation issues.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[String],
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Stable)?;
    rbmt_eprintln!("Building docs...");

    let mut cmd = cargo_cmd(sh).arg("doc").arg("--all-features").arg("--no-deps").arg("--examples");

    // Add package filters if specified.
    for package in packages {
        cmd = cmd.args(&["-p", &package.id]);
    }

    if open {
        cmd = cmd.arg("--open");
    }

    cmd.env("RUSTDOCFLAGS", "-D warnings").run_with_capture()?;

    rbmt_eprintln!("Docs built successfully.");
    Ok(())
}

/// Build documentation for docs.rs with the nightly toolchain.
///
/// This emulates the docs.rs build environment by using the nightly toolchain
/// with `--cfg docsrs` enabled. This catches docs.rs-specific issues.
pub fn run_docsrs(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[String],
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;
    rbmt_eprintln!("Building docs...");

    let mut cmd = cargo_cmd(sh).arg("doc").arg("--all-features").arg("--no-deps").arg("--examples");

    // Add package filters if specified.
    for package in packages {
        cmd = cmd.args(&["-p", &package.id]);
    }

    if open {
        cmd = cmd.arg("--open");
    }

    cmd.env("RUSTDOCFLAGS", "--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links")
        .run_with_capture()?;

    rbmt_eprintln!("Docs built successfully.");
    Ok(())
}
