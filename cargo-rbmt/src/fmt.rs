//! Code formatting tasks.

use xshell::Shell;

use crate::environment::{quiet_println, Package};
use crate::quiet_cmd;
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Format (or check the formatting of) all packages in the workspace.
pub fn run(sh: &Shell, check: bool, packages: &[Package]) -> Result<(), Box<dyn std::error::Error>> {
    prepare_toolchain(sh, Toolchain::Nightly)?;

    if check {
        quiet_println("Checking formatting...");
    } else {
        quiet_println("Formatting files...");
    }

    let mut cmd = quiet_cmd!(sh, "cargo fmt");

    if packages.is_empty() {
        cmd = cmd.arg("--all");
    } else {
        for (name, _) in packages {
            cmd = cmd.args(&["-p", name]);
        }
    }

    if check {
        cmd = cmd.arg("--check");
    }

    cmd.run()?;

    if check {
        quiet_println("Formatting check passed");
    } else {
        quiet_println("Formatting complete");
    }

    Ok(())
}
