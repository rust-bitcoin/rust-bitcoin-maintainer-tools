//! Git utilities for switching refs and enumerating commits.

use xshell::Shell;

use crate::environment::quiet_println;
use crate::rbmt_cmd;

/// RAII guard for temporarily switching git refs.
///
/// Switches to the given ref on construction and switches back to the
/// previous ref (via `git switch --detach -`) on drop.
pub struct GitSwitchGuard<'a> {
    sh: &'a Shell,
}

impl<'a> GitSwitchGuard<'a> {
    /// Create a new guard and switch to the specified ref.
    pub fn new(sh: &'a Shell, git_ref: &str) -> Result<Self, Box<dyn std::error::Error>> {
        quiet_println(&format!("Switching to ref: {}", git_ref));
        rbmt_cmd!(sh, "git switch --detach {git_ref}").run()?;
        Ok(Self { sh })
    }
}

impl Drop for GitSwitchGuard<'_> {
    fn drop(&mut self) {
        quiet_println("Returning to previous ref...");
        // Use expect here because if this fails, we're already in a bad state
        // and there's not much we can do about it in Drop.
        rbmt_cmd!(self.sh, "git switch --detach -")
            .run()
            .expect("Failed to switch back to previous git ref");
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
