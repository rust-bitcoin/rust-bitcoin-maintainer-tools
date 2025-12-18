//! Manage cargo lock files for minimal and recent dependency versions.
//!
//! Note: These commands intentionally omit `--locked` because they need to
//! generate and modify lockfiles. Using `--locked` would prevent the dependency
//! resolution we need here.

use std::fs;

use clap::ValueEnum;
use xshell::Shell;

use crate::environment::quiet_println;
use crate::quiet_cmd;
use crate::toolchain::{check_toolchain, Toolchain};

/// The standard Cargo lockfile name.
const CARGO_LOCK: &str = "Cargo.lock";
/// The temporary backup file for Cargo.lock.
const CARGO_LOCK_BACKUP: &str = "Cargo.lock.backup";

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
    pub fn filename(self) -> &'static str {
        match self {
            Self::Minimal => "Cargo-minimal.lock",
            Self::Recent => "Cargo-recent.lock",
            Self::Existing => CARGO_LOCK,
        }
    }

    /// Derive this lockfile type from dependencies and activate it as Cargo.lock.
    pub fn derive(self, sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Minimal => derive_minimal_lockfile(sh),
            Self::Recent => update_recent_lockfile(sh),
            Self::Existing => {
                // No-op, use existing Cargo.lock.
                Ok(())
            }
        }
    }

    /// Restore a previously derived lockfile to Cargo.lock.
    pub fn restore(self, sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Minimal | Self::Recent => {
                fs::copy(
                    sh.current_dir().join(self.filename()),
                    sh.current_dir().join(CARGO_LOCK),
                )?;
                Ok(())
            }
            Self::Existing => {
                // No-op, Cargo.lock is already in place.
                Ok(())
            }
        }
    }
}

/// Update Cargo-minimal.lock and Cargo-recent.lock files.
///
/// * `Cargo-minimal.lock` - Uses minimal versions that satisfy dependency constraints.
/// * `Cargo-recent.lock` - Uses recent/updated versions of dependencies.
///
/// This helps catch cases where you've specified a minimum version that's too high,
/// or where your code relies on features from newer versions than declared.
///
/// The original Cargo.lock is preserved and restored after generation in case
/// it is being tracked for publication.
pub fn run(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    check_toolchain(sh, Toolchain::Nightly)?;

    let repo_dir = sh.current_dir();
    quiet_println(&format!("Updating lock files in: {}", repo_dir.display()));

    backup_existing(sh)?;
    LockFile::Minimal.derive(sh)?;
    LockFile::Recent.derive(sh)?;
    restore_existing(sh)?;

    quiet_println("Lock files updated successfully");

    Ok(())
}

/// Derive a minimal versions lockfile.
///
/// The minimal versions strategy uses a combination of `-Z direct-minimal-versions`
/// and `-Z minimal-versions` to ensure two rules:
///
/// 1. Direct dependency versions in manifests are accurate (not bumped by transitive deps).
/// 2. The entire dependency tree uses minimal versions that still satisfy constraints.
fn derive_minimal_lockfile(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
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
    // satisfy constraints.
    quiet_println("Generating minimal versions lockfile...");
    remove_lock_file(sh)?;
    quiet_cmd!(sh, "cargo check --all-features -Z minimal-versions").run()?;

    // Save a copy to Cargo-minimal.lock for workspace tracking.
    copy_lock_file(sh, LockFile::Minimal)?;

    Ok(())
}

/// Updates or creates a recent versions lockfile.
///
/// This uses `cargo check` to conservatively update dependency versions within
/// the constraints specified in Cargo.toml. Cargo will keep existing dependencies
/// at their current versions if they still satisfy constraints, only update when
/// necessary (e.g., when adding new dependencies or constraints change).
fn update_recent_lockfile(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    quiet_println("Generating recent versions lockfile...");

    // Try to restore existing Cargo-recent.lock for conservative updates.
    // If it doesn't exist cargo check will create a fresh one.
    remove_lock_file(sh)?;
    let _ = LockFile::Recent.restore(sh);
    quiet_cmd!(sh, "cargo check --all-features").run()?;

    // Save a copy to Cargo-recent.lock for workspace tracking.
    copy_lock_file(sh, LockFile::Recent)?;

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

/// Backup the existing Cargo.lock file.
fn backup_existing(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let source = sh.current_dir().join(CARGO_LOCK);
    let backup = sh.current_dir().join(CARGO_LOCK_BACKUP);
    if source.exists() {
        fs::copy(&source, &backup)?;
    }
    Ok(())
}

/// Restore the existing Cargo.lock file from backup.
fn restore_existing(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let backup = sh.current_dir().join(CARGO_LOCK_BACKUP);
    let dest = sh.current_dir().join(CARGO_LOCK);
    if backup.exists() {
        fs::copy(&backup, &dest)?;
        fs::remove_file(&backup)?;
    }
    Ok(())
}
