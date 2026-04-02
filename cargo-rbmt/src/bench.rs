//! Benchmark testing tasks.

use xshell::Shell;

use crate::environment::{rbmt_eprintln, Package};
use crate::rbmt_cmd;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run benchmark tests for all crates in the workspace.
pub fn run(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, Toolchain::Nightly)?;

    rbmt_eprintln(&format!("Running bench tests for {} crates", packages.len()));

    for package in packages {
        rbmt_eprintln(&format!("Running bench tests in: {}", package.dir.display()));

        // Use pushd pattern to change and restore directory.
        let _dir = sh.push_dir(&package.dir);

        rbmt_cmd!(sh, "cargo --locked bench").env("RUSTFLAGS", "--cfg=bench").run()?;
    }

    Ok(())
}
