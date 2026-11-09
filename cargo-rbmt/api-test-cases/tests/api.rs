//! Integration tests for `cargo rbmt api`.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Mutex;

use xshell::{cmd, Shell};

const TEMP_BRANCH: &str = "_temp_api_diff_test";

const API_DIFF_ADDITION: &str = r#"
pub fn added_after_baseline() -> &'static str {
    "new API"
}
"#;

/// Guards the workspace's shared on-disk state.
///
/// `cargo rbmt api` mutates the workspace `Cargo.lock` (lockfile activation backs up
/// and restores a single shared `Cargo.lock.backup`), and the diff test also mutates
/// git state. Concurrent runs race on both, so every test holds this lock.
static WORKSPACE_STATE: Mutex<()> = Mutex::new(());

/// Restores the original git ref and deletes the temp branch on drop.
struct TempBranchGuard {
    orig_ref: String,
}

impl TempBranchGuard {
    fn new(sh: &Shell) -> Result<Self, Box<dyn Error>> {
        let orig_ref = cmd!(sh, "git rev-parse HEAD").read()?;
        cmd!(sh, "git switch -c {TEMP_BRANCH}").quiet().output()?;
        Ok(Self { orig_ref: orig_ref.trim().to_string() })
    }
}

impl Drop for TempBranchGuard {
    fn drop(&mut self) {
        let sh = Shell::new().unwrap();
        let orig = &self.orig_ref;
        let _ = cmd!(sh, "git switch --detach {orig}").quiet().output();
        let _ = cmd!(sh, "git branch -D {TEMP_BRANCH}").quiet().output();
    }
}

#[test]
fn api_leak_detected() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let status = cmd!(sh, "cargo rbmt api -p api-leaky").ignore_status().output().unwrap().status;
    assert_eq!(status.code(), Some(3));
}

#[test]
fn api_item_reexport_detected() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let status =
        cmd!(sh, "cargo rbmt api -p api-item-reexport").ignore_status().output().unwrap().status;
    assert_eq!(status.code(), Some(3));
}

#[test]
fn api_crate_reexport_detected() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let status =
        cmd!(sh, "cargo rbmt api -p api-crate-reexport").ignore_status().output().unwrap().status;
    assert_eq!(status.code(), Some(3));
}

#[test]
fn api_crate_reexport_package_name_detected() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let status = cmd!(sh, "cargo rbmt api -p api-crate-reexport-package-name")
        .ignore_status()
        .output()
        .unwrap()
        .status;
    assert_eq!(status.code(), Some(3));
}

#[test]
fn api_no_leak_passes() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let status = cmd!(sh, "cargo rbmt api -p api-no-leak").ignore_status().output().unwrap().status;
    assert_eq!(status.code(), Some(0));
}

#[test]
fn api_diff_detected() {
    let _state = WORKSPACE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let sh = Shell::new().unwrap();
    let _branch = TempBranchGuard::new(&sh).unwrap();

    let lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("api-diff/src/lib.rs");
    let mut content = std::fs::read_to_string(&lib_path).unwrap();
    content.push_str(API_DIFF_ADDITION);
    std::fs::write(&lib_path, content).unwrap();

    cmd!(sh, "git add {lib_path}").quiet().output().unwrap();
    cmd!(sh, "git -c user.name='RBMT Bot' -c user.email='bot@example.com' commit -m 'temp: add diff for api test'").quiet().output().unwrap();

    let status = cmd!(sh, "cargo rbmt api -p api-diff --baseline HEAD~1")
        .ignore_status()
        .output()
        .unwrap()
        .status;
    assert_eq!(status.code(), Some(2));
}
