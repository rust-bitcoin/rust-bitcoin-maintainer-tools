//! Documentation building tasks.

use xshell::Shell;

use crate::environment::Package;
use crate::quiet_cmd;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Build documentation for end users with the stable toolchain.
///
/// This verifies that `cargo doc` works correctly for users with stable Rust.
/// Uses basic rustdoc warnings to catch common documentation issues.
pub fn run(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, Toolchain::Stable)?;

    let mut cmd = quiet_cmd!(sh, "cargo --locked doc --all-features --no-deps");

    // Add package filters if specified.
    for (name, _) in packages {
        cmd = cmd.args(&["-p", name]);
    }

    cmd.env("RUSTDOCFLAGS", "-D warnings").run()?;

    Ok(())
}

/// Build documentation for docs.rs with the nightly toolchain.
///
/// This emulates the docs.rs build environment by using the nightly toolchain
/// with `--cfg docsrs` enabled. This catches docs.rs-specific issues.
pub fn run_docsrs(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, Toolchain::Nightly)?;

    let mut cmd = quiet_cmd!(sh, "cargo --locked doc --all-features --no-deps");

    // Add package filters if specified.
    for (name, _) in packages {
        cmd = cmd.args(&["-p", name]);
    }

    cmd.env("RUSTDOCFLAGS", "--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links").run()?;

    Ok(())
}
