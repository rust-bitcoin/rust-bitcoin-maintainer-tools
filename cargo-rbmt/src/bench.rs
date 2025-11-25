//! Benchmark testing tasks.

use crate::environment::{get_crate_dirs, quiet_println};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use xshell::Shell;

/// Run benchmark tests for all crates in the workspace.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;

    let crate_dirs = get_crate_dirs(sh, packages)?;

    quiet_println(&format!(
        "Running bench tests for {} crates",
        crate_dirs.len()
    ));

    for crate_dir in &crate_dirs {
        quiet_println(&format!("Running bench tests in: {}", crate_dir));

        // Use pushd pattern to change and restore directory.
        let _dir = sh.push_dir(crate_dir);

        quiet_cmd!(sh, "cargo bench")
            .env("RUSTFLAGS", "--cfg=bench")
            .run()?;
    }

    Ok(())
}
