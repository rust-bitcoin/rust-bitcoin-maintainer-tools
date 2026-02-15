//! Kani verification tasks.

use xshell::Shell;

use crate::environment::quiet_println;
use crate::quiet_cmd;

/// Run kani verification at the workspace root.
pub fn run(
    sh: &Shell,
    packages: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Running kani verification");

    let mut cmd = quiet_cmd!(sh, "cargo kani");

    for pkg in packages {
        cmd = cmd.args(&["-p", pkg]);
    }

    cmd.run()?;

    Ok(())
}
