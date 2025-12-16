//! Manage cargo lock files for minimal and recent dependency versions.
//!
//! Note: These commands intentionally omit `--locked` because they need to
//! generate and modify lockfiles. Using `--locked` would prevent the dependency
//! resolution we need here.

use crate::environment::quiet_println;
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};
use clap::ValueEnum;
use std::fs;
use xshell::Shell;

/// The standard Cargo lockfile name.
const CARGO_LOCK: &str = "Cargo.lock";

/// Represents the different types of managed lock files.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum LockFile {
    /// Uses minimal versions that satisfy dependency constraints.
    Minimal,
    /// Uses recent/updated versions of dependencies.
    #[default]
    Recent,
    /// Uses the existing Cargo.lock as-is (for binary crates).
    Existing,
}

impl LockFile {
    /// Get the filename for this lock file type.
    pub fn filename(&self) -> &'static str {
        match self {
            LockFile::Minimal => "Cargo-minimal.lock",
            LockFile::Recent => "Cargo-recent.lock",
            LockFile::Existing => CARGO_LOCK,
        }
    }
}

/// Update Cargo-minimal.lock and Cargo-recent.lock files.
///
/// * `Cargo-minimal.lock` - Uses minimal versions that satisfy dependency constraints.
/// * `Cargo-recent.lock` - Uses recent/updated versions of dependencies.
///
/// The minimal versions strategy uses a combination of `-Z direct-minimal-versions`
/// and `-Z minimal-versions` to ensure two rules.
///
/// 1. Direct dependency versions in manifests are accurate (not bumped by transitive deps).
/// 2. The entire dependency tree uses minimal versions that still satisfy constraints.
///
/// This helps catch cases where you've specified a minimum version that's too high,
/// or where your code relies on features from newer versions than declared.
pub fn run(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;

    let repo_dir = sh.current_dir();
    quiet_println(&format!("Updating lock files in: {}", repo_dir.display()));

    // The `direct-minimal-versions` and `minimal-versions` dependency resolution strategy
    // flags each have a little quirk. `direct-minimal-versions` allows transitive versions
    // to upgrade, so we are not testing against the actual minimum tree. `minimal-versions`
    // allows the direct dependency versions to resolve upward due to transitive requirements,
    // so we are not testing the manifest's versions. Combo'd together though, we can get the
    // best of both worlds to ensure the actual minimum dependencies listed in the crate
    // manifests build.

    // Check that all explicit direct dependency versions are not lying,
    // as in, they are not being bumped up by transitive dependency constraints.
    quiet_println("Checking direct minimal versions...");
    remove_lock_file(sh)?;
    quiet_cmd!(sh, "cargo check --all-features -Z direct-minimal-versions").run()?;

    // Now that our own direct dependency versions can be trusted, check
    // against the lowest versions of the dependency tree which still
    // satisfy constraints. Use this as the minimal version lock file.
    quiet_println("Generating Cargo-minimal.lock...");
    remove_lock_file(sh)?;
    quiet_cmd!(sh, "cargo check --all-features -Z minimal-versions").run()?;
    copy_lock_file(sh, LockFile::Minimal)?;

    // Conservatively bump of recent dependencies.
    quiet_println("Updating Cargo-recent.lock...");
    restore_lock_file(sh, LockFile::Recent)?;
    quiet_cmd!(sh, "cargo check --all-features").run()?;
    copy_lock_file(sh, LockFile::Recent)?;

    quiet_println("Lock files updated successfully");

    Ok(())
}

/// Remove Cargo.lock file if it exists.
fn remove_lock_file(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = sh.current_dir().join(CARGO_LOCK);
    if lock_path.exists() {
        fs::remove_file(&lock_path)?;
    }
    Ok(())
}

/// Copy Cargo.lock to a specific lock file.
fn copy_lock_file(sh: &Shell, target: LockFile) -> Result<(), Box<dyn std::error::Error>> {
    let source = sh.current_dir().join(CARGO_LOCK);
    let dest = sh.current_dir().join(target.filename());
    fs::copy(&source, &dest)?;
    Ok(())
}

/// Restore a specific lock file to Cargo.lock.
pub fn restore_lock_file(sh: &Shell, source: LockFile) -> Result<(), Box<dyn std::error::Error>> {
    // Existing uses Cargo.lock as-is, no need to restore.
    if matches!(source, LockFile::Existing) {
        return Ok(());
    }

    let src_path = sh.current_dir().join(source.filename());
    let dest_path = sh.current_dir().join(CARGO_LOCK);
    fs::copy(&src_path, &dest_path)?;
    Ok(())
}
