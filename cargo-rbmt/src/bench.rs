//! Benchmark testing tasks.

use crate::environment::{get_packages, quiet_println};
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use xshell::Shell;

/// Run benchmark tests for all crates in the workspace.
pub fn run(sh: &Shell, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;

    let package_info = get_packages(sh, packages)?;

    quiet_println(&format!(
        "Running bench tests for {} crates",
        package_info.len()
    ));

    for (_package_name, package_dir) in &package_info {
        quiet_println(&format!("Running bench tests in: {}", package_dir.display()));

        // Use pushd pattern to change and restore directory.
        let _dir = sh.push_dir(package_dir);

        quiet_cmd!(sh, "cargo --locked bench")
            .env("RUSTFLAGS", "--cfg=bench")
            .run()?;
    }

    Ok(())
}
