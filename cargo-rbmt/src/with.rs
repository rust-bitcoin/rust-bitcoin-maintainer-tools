// SPDX-License-Identifier: MIT AND Apache-2.0

//! Run a command with lockfile and toolchain management.
//! Stdio is inherited directly; the child's exit code is relayed.

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use xshell::Shell;

use crate::lock::LockFile;
use crate::toolchain::{self, Toolchain};

/// Run an arbitrary command with the specified lockfile and toolchain.
///
/// Unlike [`run`](crate::run), this does *not* resolve workspace packages, iterate commits, or
/// capture/redirect output. The child inherits stdout and stderr directly, and this function
/// returns its exit code.
///
/// # Arguments
///
/// * `args` - The program to run and its arguments (`args[0]` is the program).
///
/// # Returns
///
/// The child's exit code (`0` = success). On Unix, if the child is terminated by a signal the exit
/// code follows the POSIX shell convention of `128 + signal_number`. An `Err` is only returned if
/// the command could not be started (I/O / spawn failure).
pub fn run(
    sh: &Shell,
    lockfile: Option<LockFile>,
    toolchain: Toolchain,
    args: &[String],
) -> Result<i32, Box<dyn std::error::Error>> {
    let lockfile = if let Some(lockfile) = lockfile { lockfile } else { LockFile::max(sh)? };
    let _lockfile_guard = lockfile.activate(sh)?;

    toolchain::prepare_toolchain(sh, toolchain)?;

    let (prog, rest) = args.split_first().ok_or("no command to run")?;

    // Build the command through xshell so it picks up the shell's environment (including
    // RUSTUP_TOOLCHAIN set by prepare_toolchain) and working directory. Convert to a
    // std::process::Command to run with inherited stdio and read the exit status.
    let cmd = sh.cmd(prog).args(rest);
    let status = Command::from(cmd).status()?;
    let code = status.code().unwrap_or_else(|| {
        // When a child is terminated by a signal, code() returns None. Follow the POSIX shell
        // convention of 128 + signal_number so callers can distinguish signal termination from
        // normal exits.
        #[cfg(unix)]
        {
            128 + status.signal().unwrap_or(0)
        }
        #[cfg(not(unix))]
        {
            1
        }
    });
    Ok(code)
}
