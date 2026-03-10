use xshell::Shell;

use crate::environment::{is_quiet_mode, quiet_println};
use crate::quiet_cmd;
use crate::toolchain::{get_workspace_msrv, read_version_file};

/// Fixed components installed on every toolchain.
const COMPONENTS: &str = "rust-src,clippy,rustfmt";

/// Fixed target installed on every toolchain (for no-std cross-compilation testing).
const TARGET: &str = "thumbv7m-none-eabi";

/// Env var names exported after setup.
const ENV_NIGHTLY: &str = "RBMT_NIGHTLY";
const ENV_STABLE: &str = "RBMT_STABLE";
const ENV_MSRV: &str = "RBMT_MSRV";

/// Install all three toolchains (nightly, stable, MSRV) and export env vars.
///
/// When `update_nightly` is true, the floating `nightly` toolchain is first
/// installed, its resolved version queried from rustc, and the result written
/// to `nightly-version` before the normal install and export path runs.
///
/// When `update_stable` is true, the same is done for `stable-version`.
///
/// When `msrv` is true, print the workspace MSRV to stdout and exit without
/// installing any toolchains.
pub fn run(sh: &Shell, update_nightly: bool, update_stable: bool, msrv: bool) -> Result<(), Box<dyn std::error::Error>> {
    if msrv {
        let msrv = get_workspace_msrv(sh)?;
        println!("{}", msrv);
        return Ok(());
    }
    if update_nightly {
        install_toolchain(sh, "nightly")?;
        let version = resolve_nightly_version(sh)?;
        write_version_file(sh, "nightly-version", &version)?;
        eprintln!("Updated nightly-version: {}", version);
    }

    if update_stable {
        install_toolchain(sh, "stable")?;
        let version = resolve_stable_version(sh)?;
        write_version_file(sh, "stable-version", &version)?;
        eprintln!("Updated stable-version: {}", version);
    }

    let nightly = read_version_file(sh, "nightly-version")
        .ok_or("nightly-version file not found in repository root")?;
    let stable = read_version_file(sh, "stable-version")
        .ok_or("stable-version file not found in repository root")?;
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

/// Query the resolved nightly version string (e.g. `"nightly-2025-02-17"`) from rustc.
fn resolve_nightly_version(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let output = quiet_cmd!(sh, "rustc +nightly --verbose --version").read()?;

    // Output contains a line: "commit-date: 2025-02-17"
    let date = output
        .lines()
        .find_map(|line| line.strip_prefix("commit-date: "))
        .ok_or("Could not find commit-date in `rustc +nightly --verbose --version` output")?;

    Ok(format!("nightly-{}", date))
}

/// Query the resolved stable version string (e.g. `"1.85.0"`) from rustc.
fn resolve_stable_version(sh: &Shell) -> Result<String, Box<dyn std::error::Error>> {
    let output = quiet_cmd!(sh, "rustc +stable --version").read()?;

    // Output: "rustc 1.85.0 (4d91de4e4 2025-02-17)"
    let version = output
        .strip_prefix("rustc ")
        .and_then(|s| s.split_whitespace().next())
        .ok_or("Could not parse version from `rustc +stable --version` output")?;

    Ok(version.to_string())
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

/// Write a version string to a file in the shell's current directory, with a trailing newline.
fn write_version_file(sh: &Shell, filename: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(sh.current_dir().join(filename), format!("{}\n", version))?;
    Ok(())
}
