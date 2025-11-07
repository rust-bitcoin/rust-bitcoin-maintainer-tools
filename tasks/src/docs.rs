//! Documentation building tasks.

use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use xshell::Shell;

/// Build documentation for end users with the stable toolchain.
///
/// This verifies that `cargo doc` works correctly for users with stable Rust.
/// Uses basic rustdoc warnings to catch common documentation issues.
pub fn run(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Stable)?;

    quiet_cmd!(sh, "cargo doc --all-features")
        .env("RUSTDOCFLAGS", "-D warnings")
        .run()?;

    Ok(())
}

/// Build documentation for docs.rs with the nightly toolchain.
///
/// This emulates the docs.rs build environment by using the nightly toolchain
/// with `--cfg docsrs` enabled. This catches docs.rs-specific issues.
pub fn run_docsrs(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;

    quiet_cmd!(sh, "cargo doc --all-features")
        .env(
            "RUSTDOCFLAGS",
            "--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links",
        )
        .run()?;

    Ok(())
}
