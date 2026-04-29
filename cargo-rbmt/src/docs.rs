//! Documentation building tasks.

use xshell::Shell;

use crate::environment::{Package, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Build documentation for end users with the stable toolchain.
///
/// This verifies that `cargo doc` works correctly for users with stable Rust.
/// Uses basic rustdoc warnings to catch common documentation issues.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[Package],
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Stable)?;
    rbmt_eprintln!("Building docs...");

    let mut cmd = rbmt_cmd!(sh, "cargo --locked doc --all-features --no-deps");

    // Add package filters if specified.
    for package in packages {
        cmd = cmd.args(&["-p", &package.id]);
    }

    if open {
        cmd = cmd.arg("--open");
    }

    cmd.env("RUSTDOCFLAGS", "-D warnings").run()?;

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
    packages: &[Package],
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;
    rbmt_eprintln!("Building docs...");

    let mut cmd = rbmt_cmd!(sh, "cargo --locked doc --all-features --no-deps");

    // Add package filters if specified.
    for package in packages {
        cmd = cmd.args(&["-p", &package.id]);
    }

    if open {
        cmd = cmd.arg("--open");
    }

    cmd.env("RUSTDOCFLAGS", "--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links").run()?;

    rbmt_eprintln!("Docs built successfully.");
    Ok(())
}
