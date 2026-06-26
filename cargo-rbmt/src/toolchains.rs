// SPDX-License-Identifier: MIT AND Apache-2.0

use xshell::Shell;

use crate::environment::ProgressGuard;
use crate::toolchain::{install_toolchain, Toolchain};

/// Status string for toolchains that are not configured.
const NOT_CONFIGURED: &str = "(not configured)";

/// Install configured toolchains nightly, stable, and MSRV.
///
/// Optionally updates to latest `nightly` or `stable` version.
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
        Toolchain::Nightly.update_version(sh)?;
    }

    if update_stable {
        Toolchain::Stable.update_version(sh)?;
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
