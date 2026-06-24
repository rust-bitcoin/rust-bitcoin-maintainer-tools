// SPDX-License-Identifier: MIT AND Apache-2.0

//! Code formatting tasks.

use std::fs;

use xshell::Shell;

use crate::environment::{get_workspace_packages, CmdExt, Package, ProgressGuard};
use crate::toolchain::{prepare_toolchain, Toolchain};

/// Format (or check the formatting of) all packages in the workspace.
pub fn run(sh: &Shell, check: bool, packages: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let packages = get_workspace_packages(sh, packages)?;
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;

    if check {
        rbmt_eprintln!("Checking formatting...");
    } else {
        rbmt_eprintln!("Formatting files...");
    }

    let mut cmd = rbmt_cmd!(sh, "cargo fmt");

    if packages.is_empty() {
        cmd = cmd.arg("--all");
    } else {
        for package in &packages {
            cmd = cmd.args(&["-p", &package.name]);
        }
    }

    if check {
        cmd = cmd.arg("--check");
    }

    cmd.run_with_capture()?;

    if check {
        rbmt_eprintln!("Formatting check passed");
    } else {
        remove_trailing_whitespace(sh, &packages)?;
        rbmt_eprintln!("Formatting complete");
    }

    Ok(())
}

/// Remove trailing whitespace from tracked source files in specified packages (or all if empty).
fn remove_trailing_whitespace(
    sh: &Shell,
    packages: &[Package],
) -> Result<(), Box<dyn std::error::Error>> {
    // Collect rust files from either all tracked files or just specified packages.
    let files = if packages.is_empty() {
        rbmt_cmd!(sh, "git ls-files --cached '*.rs'").read()?
    } else {
        // Get files from each specified package directory.
        let mut all_files = Vec::new();
        for package in packages {
            let pathspec = package.dir.join("**/*.rs");
            let mut cmd = rbmt_cmd!(sh, "git ls-files --cached");
            cmd = cmd.arg(pathspec);
            let files_output = cmd.read()?;
            all_files.push(files_output);
        }
        all_files.join("\n")
    };

    if files.trim().is_empty() {
        rbmt_eprintln!("No rust files found to clean whitespace from");
        return Ok(());
    }

    for file_path in files.lines() {
        if !file_path.is_empty() {
            let content = fs::read_to_string(file_path)?;
            let cleaned = content.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n";
            fs::write(file_path, cleaned)?;
        }
    }

    Ok(())
}
