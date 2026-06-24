// SPDX-License-Identifier: MIT AND Apache-2.0

//! Manage cargo lock files for minimal and recent dependency versions.
//!
//! Note: These commands intentionally omit `--locked` because they need to
//! generate and modify lockfiles. Using `--locked` would prevent the dependency
//! resolution we need here.

use std::fs;
use std::path::PathBuf;

use clap::ValueEnum;
use xshell::Shell;

use crate::environment::{get_workspace_root, CmdExt, ProgressGuard};
use crate::toolchain::{prepare_toolchain, Toolchain};

/// The standard Cargo lockfile name.
const CARGO_LOCK: &str = "Cargo.lock";
/// The temporary backup file for Cargo.lock.
const CARGO_LOCK_BACKUP: &str = "Cargo.lock.backup";

/// RAII guard that backs up and restores the original Cargo.lock file.
pub struct LockFileGuard {
    backup_path: PathBuf,
    restore_path: PathBuf,
}

impl LockFileGuard {
    pub fn new(sh: &Shell) -> Result<Self, Box<dyn std::error::Error>> {
        let workspace_root = get_workspace_root(sh)?;
        let source = workspace_root.join(CARGO_LOCK);
        let backup = workspace_root.join(CARGO_LOCK_BACKUP);

        // Backup the existing Cargo.lock file if it exists.
        if source.exists() {
            fs::copy(&source, &backup)?;
        }

        Ok(Self { backup_path: backup, restore_path: source })
    }
}

impl Drop for LockFileGuard {
    fn drop(&mut self) {
        // Restore the existing Cargo.lock file from backup (best effort).
        if self.backup_path.exists() {
            if let Err(e) = fs::copy(&self.backup_path, &self.restore_path) {
                eprintln!("Warning: Failed to restore Cargo.lock from backup: {}", e);
                return;
            }
            if let Err(e) = fs::remove_file(&self.backup_path) {
                eprintln!("Warning: Failed to remove Cargo.lock backup: {}", e);
            }
        }
    }
}

/// Represents the different types of managed lock files.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum LockFile {
    /// Minimal (oldest) dependency versions that satisfy dependency constraints.
    Minimal,
    /// Maximum (newest) dependency versions that satisfy dependency constraints.
    Maximum,
    /// Recent (conservatively updated) dependency versions that satisfy dependency constraints.
    #[default]
    Recent,
    /// `Cargo.lock` as-is (useful for binary crates).
    Existing,
}

/// Lock file types that can be generated via CLI (excludes `Existing`).
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum GeneratableLockFile {
    /// Uses minimal versions that satisfy dependency constraints.
    Minimal,
    /// Uses maximum versions that satisfy dependency constraints.
    Maximum,
    /// Uses recent/updated versions of dependencies.
    #[default]
    Recent,
}

impl From<GeneratableLockFile> for LockFile {
    fn from(lockfile: GeneratableLockFile) -> Self {
        match lockfile {
            GeneratableLockFile::Minimal => Self::Minimal,
            GeneratableLockFile::Maximum => Self::Maximum,
            GeneratableLockFile::Recent => Self::Recent,
        }
    }
}

impl LockFile {
    /// Get the filename for this lock file type.
    pub fn filename(self) -> &'static str {
        match self {
            Self::Minimal => "Cargo-minimal.lock",
            Self::Maximum => "Cargo-maximum.lock",
            Self::Recent => "Cargo-recent.lock",
            Self::Existing => CARGO_LOCK,
        }
    }

    /// Derive this lockfile type from dependencies and activate it as Cargo.lock.
    pub fn derive(self, sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Minimal => derive_minimal_lockfile(sh),
            Self::Maximum => derive_maximum_lockfile(sh),
            Self::Recent => update_recent_lockfile(sh),
            Self::Existing => {
                // No-op, use existing Cargo.lock.
                Ok(())
            }
        }
    }

    /// Restore a previously derived lockfile to Cargo.lock.
    fn restore(self, sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Minimal | Self::Maximum | Self::Recent => {
                let workspace_root = get_workspace_root(sh)?;
                let source = workspace_root.join(self.filename());
                let dest = workspace_root.join(CARGO_LOCK);

                fs::copy(&source, &dest).map_err(|e| -> Box<dyn std::error::Error> {
                    format!(
                        "Failed to restore {} lockfile (workspace: {:?}, from: {:?}, to: {:?}): {}",
                        self.filename(),
                        workspace_root,
                        source,
                        dest,
                        e
                    )
                    .into()
                })?;
                Ok(())
            }
            Self::Existing => {
                // No-op, Cargo.lock is already in place.
                Ok(())
            }
        }
    }

    /// Activate this lockfile and return a guard that restores the original on drop.
    ///
    /// This creates a backup of the current `Cargo.lock`, then copies the specified
    /// lockfile variant to `Cargo.lock`. When the returned guard is dropped, the original
    /// `Cargo.lock` is automatically restored.
    pub fn activate(self, sh: &Shell) -> Result<LockFileGuard, Box<dyn std::error::Error>> {
        let guard = LockFileGuard::new(sh)?;
        self.restore(sh)?;
        Ok(guard)
    }
}

/// Update lock files for dependency version testing.
///
/// * `Cargo-minimal.lock` - Uses minimal versions that satisfy dependency constraints.
/// * `Cargo-maximum.lock` - Uses maximum versions that satisfy dependency constraints.
/// * `Cargo-recent.lock` - Uses recent/updated versions of dependencies.
///
/// This helps catch cases where you've specified a minimum version that's too high,
/// where your code relies on features from newer versions than declared, or where
/// your code breaks with newer versions of dependencies.
///
/// The original Cargo.lock is preserved and restored after generation in case
/// it is being tracked for publication.
///
/// # Arguments
///
/// * `lockfiles` - Lock file types to generate (minimal, maximum, recent).
pub fn run(
    sh: &Shell,
    lockfiles: &[GeneratableLockFile],
) -> Result<(), Box<dyn std::error::Error>> {
    let _progress = ProgressGuard::new();
    prepare_toolchain(sh, Toolchain::Nightly)?;

    let workspace_root = get_workspace_root(sh)?;
    rbmt_eprintln!("Updating lock files in: {}", workspace_root.display());

    // Create guard to back up and ensure restoration, even on error.
    let _lockfile_guard = LockFileGuard::new(sh)?;
    for &lockfile in lockfiles {
        LockFile::from(lockfile).derive(sh)?;
    }

    rbmt_eprintln!("Lock files updated successfully");
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
    rbmt_eprintln!("Checking direct minimal versions...");
    remove_lock_file(sh)?;
    rbmt_cmd!(sh, "cargo check --all-features -Z direct-minimal-versions").run_with_capture()?;

    // Now that our own direct dependency versions can be trusted, check
    // against the lowest versions of the dependency tree which still
    // satisfy constraints.
    rbmt_eprintln!("Generating minimal versions lockfile...");
    remove_lock_file(sh)?;
    rbmt_cmd!(sh, "cargo check --all-features -Z minimal-versions").run_with_capture()?;

    // Save a copy to Cargo-minimal.lock for workspace tracking.
    copy_lock_file(sh, LockFile::Minimal)?;

    Ok(())
}

/// Derive a maximum versions lockfile.
///
/// This generates a lockfile using the highest versions of all dependencies
/// that still satisfy the constraints specified in Cargo.toml. This helps
/// catch compatibility issues with newer versions of dependencies.
fn derive_maximum_lockfile(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Generating maximum versions lockfile...");

    // Remove existing lock file and generate a fresh one with maximum compatible versions.
    remove_lock_file(sh)?;
    rbmt_cmd!(sh, "cargo generate-lockfile").run_with_capture()?;

    // Save a copy to Cargo-maximum.lock for workspace tracking.
    copy_lock_file(sh, LockFile::Maximum)?;

    Ok(())
}

/// Updates or creates a recent versions lockfile.
///
/// This uses `cargo check` to conservatively update dependency versions within
/// the constraints specified in Cargo.toml. Cargo will keep existing dependencies
/// at their current versions if they still satisfy constraints, only update when
/// necessary (e.g., when adding new dependencies or constraints change).
fn update_recent_lockfile(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    rbmt_eprintln!("Generating recent versions lockfile...");

    // Try to restore existing Cargo-recent.lock for conservative updates.
    // If it doesn't exist cargo check will create a fresh one.
    remove_lock_file(sh)?;
    let _ = LockFile::Recent.restore(sh);
    rbmt_cmd!(sh, "cargo check --all-features").run_with_capture()?;

    // Save a copy to Cargo-recent.lock for workspace tracking.
    copy_lock_file(sh, LockFile::Recent)?;

    Ok(())
}

/// Remove Cargo.lock file if it exists.
fn remove_lock_file(sh: &Shell) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = get_workspace_root(sh)?.join(CARGO_LOCK);
    if lock_path.exists() {
        fs::remove_file(&lock_path)?;
    }
    Ok(())
}

/// Copy Cargo.lock to a specific lock file.
fn copy_lock_file(sh: &Shell, target: LockFile) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = get_workspace_root(sh)?;
    fs::copy(workspace_root.join(CARGO_LOCK), workspace_root.join(target.filename()))?;
    Ok(())
}
