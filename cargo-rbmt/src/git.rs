//! Git utilities for switching refs and enumerating commits.

use std::fmt;

use xshell::Shell;

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
pub struct GitSwitchGuard<'a> {
    sh: &'a Shell,
    original_ref: Ref,
}

impl<'a> GitSwitchGuard<'a> {
    /// Create a new guard and switch to the specified ref.
    pub fn new(sh: &'a Shell, git_ref: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let original_ref = Ref::current(sh)?;
        rbmt_eprintln!("Switching from {} to {}", original_ref, git_ref);
        rbmt_cmd!(sh, "git switch --detach").arg(git_ref).run()?;
        Ok(Self { sh, original_ref })
    }
}

impl Drop for GitSwitchGuard<'_> {
    fn drop(&mut self) {
        rbmt_eprintln!("Returning to original ref {}", self.original_ref);

        let git_switch = match &self.original_ref {
            Ref::Branch(name) => {
                // For branches, use normal switch (no --detach).
                rbmt_cmd!(self.sh, "git switch").arg(name)
            }
            Ref::Commit(sha) => {
                // For commits, use --detach to enter detached HEAD state.
                rbmt_cmd!(self.sh, "git switch --detach").arg(sha)
            }
        };

        // Panic on failure because we're in a bad state.
        git_switch.run().expect("Failed to switch back to previous ref");
    }
}

/// List the commits between the given base ref and HEAD, oldest first.
pub fn list_commits(sh: &Shell, base: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let range_base = rbmt_cmd!(sh, "git merge-base HEAD {base}").read()?;
    let range_base = range_base.trim();
    let output = rbmt_cmd!(sh, "git log --reverse --format=%H {range_base}..HEAD").read()?;
    let commits = output.lines().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
    Ok(commits)
}
