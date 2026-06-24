// SPDX-License-Identifier: MIT AND Apache-2.0

//! Documentation building tasks.

use xshell::Shell;

use crate::environment::{cargo_cmd, get_workspace_packages, CmdExt, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Documentation build mode.
#[derive(Debug, Clone, Copy)]
pub enum DocsMode {
    /// Build documentation for end users with the stable toolchain.
    ///
    /// This verifies that `cargo doc` works correctly for users with stable Rust.
    /// Uses basic rustdoc warnings to catch common documentation issues.
    Docs,
    /// Build documentation for docs.rs with the nightly toolchain.
    ///
    /// This emulates the docs.rs build environment by using the nightly toolchain
    /// with `--cfg docsrs` enabled. This catches docs.rs-specific issues.
    DocsRs,
}

impl DocsMode {
    /// Returns the toolchain to use for this mode.
    fn toolchain(self) -> Toolchain {
        match self {
            Self::Docs => Toolchain::Stable,
            Self::DocsRs => Toolchain::Nightly,
        }
    }

    /// Returns the `RUSTDOCFLAGS` environment variable value for this mode.
    fn rustdocflags(self) -> &'static str {
        match self {
            Self::Docs => "-D warnings",
            // The `docsrs` configuration is passed by cargo when building for the docs.rs server,
            // manually enabling here to test docs which are docs.rs only.
            Self::DocsRs => "--cfg docsrs -D warnings -D rustdoc::broken-intra-doc-links",
        }
    }
}

/// Build documentation for the specified packages.
///
/// # Arguments
///
/// * `sh` - The shell context.
/// * `lockfile` - The lockfile for dependency versions.
/// * `packages` - Packages to document, empty for all.
/// * `mode` - Documentation build mode (normal or docs.rs).
/// * `open` - Whether to open the documentation in a browser after building.
pub fn run(
    sh: &Shell,
    lockfile: LockFile,
    packages: &[String],
    mode: DocsMode,
    open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, mode.toolchain())?;
    rbmt_eprintln!("Building docs...");

    let mut cmd = cargo_cmd(sh)
        .arg("doc")
        .arg("--all-features")
        .arg("--no-deps")
        .env("RUSTDOCFLAGS", mode.rustdocflags());

    // Add package filters if specified.
    for package in packages {
        cmd = cmd.args(&["-p", &package.id]);
    }

    if open {
        cmd = cmd.arg("--open");
    }

    cmd.run_with_capture()?;

    rbmt_eprintln!("Docs built successfully.");
    Ok(())
}
