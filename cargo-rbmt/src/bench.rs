//! Benchmark testing tasks.

use xshell::Shell;

use crate::environment::{OutputMode, Package, ProgressGuard};
use crate::lock::LockFile;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Run benchmark tests for all crates in the workspace.
pub fn run(sh: &Shell, lockfile: LockFile, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    let _lockfile_guard = lockfile.activate(sh)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;
    rbmt_eprintln!("Running bench tests for {} crates", packages.len());

    for package in packages {
        rbmt_eprintln!("Running bench tests in: {}", package.dir.display());

        // Use pushd pattern to change and restore directory.
        let _dir = sh.push_dir(&package.dir);

        // Capture output and show in stdout for verbose mode.
        let output =
            rbmt_cmd!(sh, "cargo --locked bench").env("RUSTFLAGS", "--cfg=bench").read()?;
        if matches!(OutputMode::from_env(), OutputMode::Verbose) {
            println!("{}", output);
        }
    }

    rbmt_eprintln!("Benches complete.");
    Ok(())
}
