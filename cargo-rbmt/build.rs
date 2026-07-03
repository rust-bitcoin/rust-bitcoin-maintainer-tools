// SPDX-License-Identifier: MIT AND Apache-2.0

use std::process::Command;

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string());

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let build_version =
        if let Some(hash) = &git_hash { format!("{} ({})", version, hash) } else { version };

    println!("cargo:rustc-env=RBMT_BUILD_VERSION={}", build_version);
    println!("cargo:rustc-env=RBMT_GIT_HASH={}", git_hash.unwrap_or_default());

    // Optimize re-builds by only rebuilding if HEAD
    // is updated or the build script itself is changed.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
}
