use xshell::Shell;

use crate::environment::ProgressGuard;
use crate::toolchain::{get_workspace_msrv, Toolchain};

/// Fixed components installed on every toolchain.
const COMPONENTS: &str = "rust-src,clippy,rustfmt";

/// Fixed target installed on every toolchain (for no-std cross-compilation testing).
const TARGET: &str = "thumbv7m-none-eabi";

/// Install all three toolchains (nightly, stable, MSRV) and optionally print versions.
///
/// When `update_nightly` is true, the floating `nightly` toolchain is first
/// installed, its resolved version queried from rustc, and the result written
/// to `nightly-version` before the normal install path runs. When `update_stable` is
/// true, the same is done for `stable-version`.
///
/// When `msrv`, `nightly`, or `stable` is true, print the correspoinding version
/// to stdout and exit without installing any toolchains.
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
        let msrv = get_workspace_msrv(sh)?;
        println!("{}", msrv);
        return Ok(());
    }

    if nightly {
        let nightly_version = Toolchain::Nightly.read_version(sh)?;
        println!("{}", nightly_version);
        return Ok(());
    }

    if stable {
        let stable_version = Toolchain::Stable.read_version(sh)?;
        println!("{}", stable_version);
        return Ok(());
    }

    if update_nightly {
        install_toolchain(sh, "nightly")?;
        let version = resolve_nightly_version(sh)?;
        Toolchain::Nightly.write_version(sh, &version)?;
        rbmt_eprintln!("Updated nightly-version: {}", version);
    }

    if update_stable {
        install_toolchain(sh, "stable")?;
        let version = resolve_stable_version(sh)?;
        Toolchain::Stable.write_version(sh, &version)?;
        rbmt_eprintln!("Updated stable-version: {}", version);
    }

    let nightly_version = Toolchain::Nightly.read_version(sh)?;
    let stable_version = Toolchain::Stable.read_version(sh)?;
    let msrv_version = get_workspace_msrv(sh)?;

    install_toolchain(sh, &nightly_version)?;
    install_toolchain(sh, &stable_version)?;
    install_toolchain(sh, &msrv_version)?;

    rbmt_eprintln!(
        "Installed toolchains: nightly={}, stable={}, msrv={}",
        nightly_version,
        stable_version,
        msrv_version
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

/// Install a single toolchain with the fixed components and target.
fn install_toolchain(sh: &Shell, toolchain: &str) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Installing toolchain {}", toolchain);

    rbmt_cmd!(
        sh,
        "rustup toolchain install {toolchain} --component {COMPONENTS} --target {TARGET} --no-self-update"
    )
    // Always suppress stdout so that only the `export` statements printed by
    // [`run`] reach stdout. This matters because the caller does
    // `eval "$(cargo rbmt toolchains)"`, and any stray rustup stdout would be
    // passed to `eval`.
    .ignore_stdout()
    .run()?;
    Ok(())
}
