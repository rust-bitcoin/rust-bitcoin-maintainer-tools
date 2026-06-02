// SPDX-License-Identifier: MIT AND Apache-2.0

use xshell::Shell;

use crate::environment::ProgressGuard;
use crate::toolchain::{install_toolchain, reinstall_toolchain, Toolchain};

/// Status string for toolchains that are not configured.
const NOT_CONFIGURED: &str = "(not configured)";

/// Install configured toolchains nightly, stable, and MSRV.
///
/// When `update_nightly` is true, the floating `nightly` toolchain is first
/// installed, its resolved version queried from rustc, and the result written
/// to `nightly-version` before the normal install path runs. When `update_stable` is
/// true, the same is done for `stable-version`.
///
/// When `msrv`, `nightly`, or `stable` is true, print the corresponding version
/// to stdout and exit without installing any toolchains. If a requested version is
/// not configured, it is not printed (silent handling).
///
/// During normal installation, any missing toolchain configurations trigger a warning
/// but do not prevent installation of the remaining toolchains.
#[allow(clippy::fn_params_excessive_bools)]
pub fn run(
    sh: &Shell,
    update_nightly: bool,
    update_stable: bool,
    msrv: bool,
    nightly: bool,
    stable: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();
    if msrv {
        if let Some(msrv) = Toolchain::Msrv.try_read_version(sh) {
            println!("{}", msrv);
        }
        return Ok(());
    }

    if nightly {
        if let Some(nightly_version) = Toolchain::Nightly.try_read_version(sh) {
            println!("{}", nightly_version);
        }
        return Ok(());
    }

    if stable {
        if let Some(stable_version) = Toolchain::Stable.try_read_version(sh) {
            println!("{}", stable_version);
        }
        return Ok(());
    }

    if update_nightly {
        if install_toolchain(sh, "nightly").is_err() {
            rbmt_eprintln!("Install failed, retrying with reinstall");
            reinstall_toolchain(sh, "nightly")?;
        }
        let version = resolve_nightly_version(sh)?;
        Toolchain::Nightly.write_version(sh, &version)?;
        rbmt_eprintln!("Updated nightly-version: {}", version);
    }

    if update_stable {
        if install_toolchain(sh, "stable").is_err() {
            rbmt_eprintln!("Install failed, retrying with reinstall");
            reinstall_toolchain(sh, "stable")?;
        }
        let version = resolve_stable_version(sh)?;
        Toolchain::Stable.write_version(sh, &version)?;
        rbmt_eprintln!("Updated stable-version: {}", version);
    }

    let nightly_status = if let Some(version) = Toolchain::Nightly.try_read_version(sh) {
        install_toolchain(sh, &version)?;
        version
    } else {
        rbmt_eprintln!("No pinned nightly toolchain found in [workspace.metadata.rbmt.toolchains] or [package.metadata.rbmt.toolchains]");
        NOT_CONFIGURED.to_string()
    };

    let stable_status = if let Some(version) = Toolchain::Stable.try_read_version(sh) {
        install_toolchain(sh, &version)?;
        version
    } else {
        rbmt_eprintln!("No pinned stable toolchain found in [workspace.metadata.rbmt.toolchains] or [package.metadata.rbmt.toolchains]");
        NOT_CONFIGURED.to_string()
    };

    let msrv_status = if let Some(version) = Toolchain::Msrv.try_read_version(sh) {
        install_toolchain(sh, &version)?;
        version
    } else {
        rbmt_eprintln!("No MSRV (rust-version) found in any Cargo.toml in the workspace");
        NOT_CONFIGURED.to_string()
    };

    rbmt_eprintln!(
        "Toolchain installation complete: nightly={}, stable={}, msrv={}",
        nightly_status,
        stable_status,
        msrv_status
    );

    Ok(())
}

/// Query the resolved nightly version string (e.g. `"nightly-2025-02-17"`) from rustc.
fn resolve_nightly_version(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let output = rbmt_cmd!(sh, "rustc +nightly --verbose --version").read()?;

    // Output contains a line: "commit-date: 2025-02-17"
    let date = output
        .lines()
        .find_map(|line| line.strip_prefix("commit-date: "))
        .ok_or("Could not find commit-date in `rustc +nightly --verbose --version` output")?;

    Ok(format!("nightly-{}", date))
}

/// Query the resolved stable version string (e.g. `"1.85.0"`) from rustc.
fn resolve_stable_version(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let output = rbmt_cmd!(sh, "rustc +stable --version").read()?;

    // Output: "rustc 1.85.0 (4d91de4e4 2025-02-17)"
    let version = output
        .strip_prefix("rustc ")
        .and_then(|s| s.split_whitespace().next())
        .ok_or("Could not parse version from `rustc +stable --version` output")?;

    Ok(version.to_string())
}
