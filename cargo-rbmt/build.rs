// SPDX-License-Identifier: MIT AND Apache-2.0

use std::process::Command;

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let build_version = format!("{} ({})", version, git_hash);

    println!("cargo:rustc-env=RBMT_BUILD_VERSION={}", build_version);

    // Optimize re-builds by only rebuilding if HEAD
    // is updated or the build script itself is changed.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
}
