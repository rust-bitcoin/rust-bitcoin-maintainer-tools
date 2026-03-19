//! Benchmark testing tasks.

use xshell::Shell;

use crate::environment::{quiet_println, Package};
use crate::quiet_cmd;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run benchmark tests for all crates in the workspace.
pub fn run(sh: &Shell, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, Toolchain::Nightly)?;

    quiet_println(&format!("Running bench tests for {} crates", packages.len()));

    for package in packages {
        quiet_println(&format!("Running bench tests in: {}", package.dir.display()));

        // Use pushd pattern to change and restore directory.
        let _dir = sh.push_dir(&package.dir);

        quiet_cmd!(sh, "cargo --locked bench").env("RUSTFLAGS", "--cfg=bench").run()?;
    }

    Ok(())
}
