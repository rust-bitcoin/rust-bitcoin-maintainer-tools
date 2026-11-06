//! Test runner for the `cargo rbmt api` command.

use std::error::Error;
use std::path::PathBuf;
use std::process::{ExitCode, ExitStatus};

use xshell::{cmd, Shell};

/// Temporary git branch used to test synthetic diff.
const TEMP_BRANCH: &str = "_temp_api_diff_test";

/// API addition appended to `api-diff` to create a synthetic diff for testing.
const API_DIFF_ADDITION: &str = r#"
pub fn added_after_baseline() -> &'static str {
    "new API"
}
"#;

/// Test to run.
struct Test {
    name: &'static str,
    expected_exit: i32,
    run: fn(&Shell) -> Result<ExitStatus, Box<dyn Error>>,
}

/// Restores the original git ref and deletes the temporary branch on drop.
struct TempBranchGuard {
    orig_ref: String,
}

impl TempBranchGuard {
    fn new(sh: &Shell) -> Result<Self, Box<dyn Error>> {
        let orig_ref = cmd!(sh, "git rev-parse HEAD").read()?;
        cmd!(sh, "git switch -c {TEMP_BRANCH}").run()?;
        Ok(Self { orig_ref: orig_ref.trim().to_string() })
    }
}

impl Drop for TempBranchGuard {
    fn drop(&mut self) {
        let sh = Shell::new().unwrap();
        let orig = &self.orig_ref;
        let _ = cmd!(sh, "git switch --detach {orig}").run();
        let _ = cmd!(sh, "git branch -D {TEMP_BRANCH}").run();
    }
}

/// Verifies exit code 1 when a package's public API leaks a private dependency.
fn test_private_dep_leak(sh: &Shell) -> Result<ExitStatus, Box<dyn Error>> {
    let output =
        cmd!(sh, "cargo rbmt api -p api-leaky").ignore_status().output()?;
    Ok(output.status)
}

/// Verifies exit code 1 when a public re-export leaks a private dependency.
fn test_item_reexport(sh: &Shell) -> Result<ExitStatus, Box<dyn Error>> {
    let output =
        cmd!(sh, "cargo rbmt api -p api-item-reexport").ignore_status().output()?;
    Ok(output.status)
}

fn test_crate_reexport(sh: &Shell) -> Result<ExitStatus, Box<dyn Error>> {
    let output =
        cmd!(sh, "cargo rbmt api -p api-crate-reexport").ignore_status().output()?;
    Ok(output.status)
}

/// Verifies `cargo rbmt api --baseline` has exit code 2 when the API changed.
fn test_api_diff(sh: &Shell) -> Result<ExitStatus, Box<dyn Error>> {
    let _guard = TempBranchGuard::new(sh)?;

    // Append the API addition to api-diff's lib.rs.
    let lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../api-diff/src/lib.rs");
    let mut content = std::fs::read_to_string(&lib_path)?;
    content.push_str(API_DIFF_ADDITION);
    std::fs::write(&lib_path, content)?;

    // Commit the change.
    cmd!(sh, "git add {lib_path}").run()?;
    cmd!(sh, "git commit -m 'temp: add diff for api test'").run()?;

    let output = cmd!(sh, "cargo rbmt api -p api-diff --baseline HEAD~1")
        .ignore_status()
        .output()?;

    Ok(output.status)
}

fn main() -> ExitCode {
    let sh = Shell::new().unwrap();

    let tests = [
        Test { name: "API leak", expected_exit: 1, run: test_private_dep_leak },
        Test { name: "API item re-export", expected_exit: 1, run: test_item_reexport },
        Test { name: "API crate re-export", expected_exit: 1, run: test_crate_reexport },
        Test { name: "API diff", expected_exit: 2, run: test_api_diff },
    ];

    if tests.iter().fold(true, |acc, test| {
        eprintln!("--- {} (expect exit {}) ---", test.name, test.expected_exit);
        let passed = match (test.run)(&sh) {
            Ok(status) => match status.code() {
                Some(code) if code == test.expected_exit => {
                    eprintln!("  PASS");
                    true
                }
                Some(code) => {
                    eprintln!("  FAIL: expected exit {}, got {code}", test.expected_exit);
                    false
                }
                None => {
                    eprintln!("  FAIL: process terminated by signal");
                    false
                }
            },
            Err(e) => {
                eprintln!("  FAIL: {e}");
                false
            }
        };
        acc && passed
    }) {
        return ExitCode::SUCCESS;
    }

    ExitCode::from(1)
}
