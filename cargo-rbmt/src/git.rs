// SPDX-License-Identifier: MIT AND Apache-2.0

//! Git utilities for switching refs and enumerating commits.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use xshell::Shell;

use crate::cleanup;
use crate::environment::get_workspace_root;

/// A git reference.
#[derive(Debug, Clone)]
enum Ref {
    /// A symbolic branch name.
    Branch(String),
    /// A commit SHA.
    Commit(String),
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Branch(name) => write!(f, "{}", name),
            Self::Commit(sha) => write!(f, "{}", sha),
        }
    }
}

impl Ref {
    /// Get the current HEAD ref, preferring branch name if on a branch, otherwise commit SHA.
    fn current(sh: &Shell) -> Result<Self, Box<dyn std::error::Error>> {
        // Try to get the current branch name (only works if attached).
        if let Ok(branch) = rbmt_cmd!(sh, "git symbolic-ref -q --short HEAD").read() {
            return Ok(Self::Branch(branch.trim().to_string()));
        }

        // If not on a branch (detached), fall back to commit SHA.
        let sha = rbmt_cmd!(sh, "git rev-parse HEAD").read()?;
        Ok(Self::Commit(sha.trim().to_string()))
    }
}

/// RAII guard for temporarily switching git refs.
///
/// Switches to the given ref on construction and switches back to the
/// original ref on drop, preserving whether you were on a branch or detached.
///
/// Registers a signal-time cleanup so the original ref is restored even when
/// the process is terminated by a signal and `Drop` does not run.
pub struct GitSwitchGuard {
    repo_root: PathBuf,
    original_ref: Ref,
    // Signal is deregistered after drop, leaving no window for neither to run.
    _registration: cleanup::Registration,
}

impl GitSwitchGuard {
    /// Switch the repository at `repo_root` to the given ref.
    ///
    /// Uses `std::process::Command` directly (rather than xshell) so it can
    /// also run from the signal-time cleanup path, which requires `'static`
    /// state. Shared between [`Drop`] and the signal-time cleanup, so it
    /// takes the ref and path directly rather than `&self`.
    fn switch_ref(repo_root: &Path, git_ref: &Ref) -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::new("git");
        cmd.current_dir(repo_root).arg("switch");
        match git_ref {
            // For branches, use normal switch (no --detach).
            Ref::Branch(name) => {
                cmd.arg(name);
            }
            // For commits, use --detach to enter detached HEAD state.
            Ref::Commit(sha) => {
                cmd.arg("--detach").arg(sha);
            }
        }
        // Capture output so a successful switch is silent, but stderr is there message on failure.
        let output = cmd.output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git switch to {} failed: {}\n{}",
                git_ref,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
            .into())
        }
    }

    /// Create a new guard and switch to the specified ref.
    pub fn new(sh: &Shell, git_ref: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let original_ref = Ref::current(sh)?;
        let repo_root = get_workspace_root(sh)?;
        rbmt_eprintln!("Switching from {} to {}", original_ref, git_ref);
        rbmt_cmd!(sh, "git switch --detach").arg(git_ref).run()?;

        let registration = cleanup::register({
            let repo_root = repo_root.clone();
            let original_ref = original_ref.clone();
            move || {
                if let Err(e) = Self::switch_ref(&repo_root, &original_ref) {
                    eprintln!(
                        "Warning: {}. You may need to run `git switch {}` manually.",
                        e, original_ref
                    );
                }
            }
        });

        Ok(Self { repo_root, original_ref, _registration: registration })
    }
}

impl Drop for GitSwitchGuard {
    fn drop(&mut self) {
        rbmt_eprintln!("Returning to original ref {}", self.original_ref);

        // Panic on failure because we're in a bad state.
        Self::switch_ref(&self.repo_root, &self.original_ref)
            .expect("Failed to switch back to previous ref");
    }
}

/// Get the current git commit ID.
///
/// Returns `None` if the working directory is not inside a git repository or
/// if git is not available.
pub fn current_commit_id(sh: &Shell) -> Option<String> {
    sh.cmd("git").args(["rev-parse", "HEAD"]).quiet().read().ok().map(|s| s.trim().to_owned())
}

/// Returns `true` if any file under the given path differs from the baseline git ref.
pub fn has_changes_since(
    sh: &Shell,
    baseline: &str,
    path: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let output = rbmt_cmd!(sh, "git diff --name-only {baseline} -- {path}").read()?;
    Ok(!output.trim().is_empty())
}

/// List the commits between the given base ref and HEAD, oldest first.
pub fn list_commits(sh: &Shell, base: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let range_base = rbmt_cmd!(sh, "git merge-base HEAD {base}").read()?;
    let range_base = range_base.trim();
    let output = rbmt_cmd!(sh, "git log --reverse --format=%H {range_base}..HEAD").read()?;
    let commits = output.lines().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
    Ok(commits)
}

/// Iterate over commits between baseline and HEAD, running an operation on each.
///
/// # Arguments
///
/// * `sh` - The shell environment.
/// * `lockfile` - Which lockfile variant to use for each commit.
/// * `baseline` - Optional baseline ref. If `None`, runs once at HEAD.
/// * `on_commit` - Closure to run on each commit. Receives the Shell and runs with git and
///   lockfile state properly configured.
pub fn for_each_commit<F>(
    sh: &Shell,
    lockfile: crate::lock::LockFile,
    baseline: Option<&str>,
    mut on_commit: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&Shell) -> Result<(), Box<dyn std::error::Error>>,
{
    if let Some(baseline) = baseline {
        let commits = list_commits(sh, baseline)?;
        if commits.is_empty() {
            rbmt_eprintln!("No commits found between '{}' and HEAD.", baseline);
            return Ok(());
        }

        for sha in commits {
            rbmt_eprintln!("Running on commit {}...", &sha[..12]);
            let _git_guard = GitSwitchGuard::new(sh, &sha)?;
            let _lockfile_guard = lockfile.activate(sh)?;

            on_commit(sh)?;
        }
    } else {
        let _lockfile_guard = lockfile.activate(sh)?;
        on_commit(sh)?;
    }

    Ok(())
}
