use std::path::Path;

use xshell::Shell;

use crate::environment::{is_quiet_mode, quiet_println};
use crate::quiet_cmd;
use crate::toolchain::get_workspace_msrv;

/// Fixed components installed on every toolchain.
const COMPONENTS: &str = "rust-src,clippy,rustfmt";

/// Fixed target installed on every toolchain (for no-std cross-compilation testing).
const TARGET: &str = "thumbv7m-none-eabi";

/// Env var names exported after setup.
const ENV_NIGHTLY: &str = "RBMT_NIGHTLY";
const ENV_STABLE: &str = "RBMT_STABLE";
const ENV_MSRV: &str = "RBMT_MSRV";

/// Install all three toolchains (nightly, stable, MSRV) and export env vars.
pub fn run(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let nightly = read_version_file("nightly-version").unwrap_or_else(|| "nightly".to_string());
    let stable = read_version_file("stable-version").unwrap_or_else(|| "stable".to_string());
    let msrv = get_workspace_msrv(sh)?;

    quiet_println(&format!(
        "Installing toolchains: nightly={}, stable={}, msrv={}",
        nightly, stable, msrv
    ));

    install_toolchain(sh, &nightly)?;
    install_toolchain(sh, &stable)?;
    install_toolchain(sh, &msrv)?;

    // Print export statements to stdout.
    println!("export {}={}", ENV_NIGHTLY, nightly);
    println!("export {}={}", ENV_STABLE, stable);
    println!("export {}={}", ENV_MSRV, msrv);

    Ok(())
}

/// Install a single toolchain with the fixed components and target.
fn install_toolchain(sh: &Shell, toolchain: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = quiet_cmd!(
        sh,
        "rustup toolchain install {toolchain} --component {COMPONENTS} --target {TARGET} --no-self-update"
    );
    // Rustup writes its `info:` lines directly to stderr, bypassing any stdout
    // capture. Suppress them in quiet mode.
    if is_quiet_mode() {
        cmd = cmd.ignore_stderr();
    }
    // Always suppress stdout so that only the `export` statements printed by
    // [`run`] reach stdout. This matters because the caller does
    // `eval "$(cargo rbmt toolchains)"`, and any stray rustup stdout would be
    // passed to `eval`.
    cmd.ignore_stdout().run()?;
    Ok(())
}

/// Read a version file from the current directory, trimming whitespace.
fn read_version_file(filename: &str) -> Option<String> {
    let path = Path::new(filename);
    if path.exists() {
        std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    }
}
