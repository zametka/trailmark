//! Git reads. trailmark never commits, stages, stashes, or pushes.
//! A fixture inside another repository must not see that parent: the git
//! directory has to live under the root we were given.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// True when `root` is the top of its own work tree: a checkout, or a linked worktree whose
/// git directory lives under the main checkout's `.git/worktrees/` (the old test, "the
/// git directory is under the root", read every worktree as someone else's repository, so
/// history checks and the shared reservation ledger were off in every loop's worktree).
pub fn own_repo(root: &Path) -> bool {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if top.is_empty() {
        return false;
    }
    match (std::fs::canonicalize(&top), std::fs::canonicalize(root)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The git directory every worktree of this repository shares (`git rev-parse
/// --git-common-dir`), or `None` outside an own repository.
pub fn common_dir(root: &Path) -> Option<PathBuf> {
    if !own_repo(root) {
        return None;
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-common-dir"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dir.is_empty() {
        return None;
    }
    let path = PathBuf::from(&dir);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    std::fs::canonicalize(path).ok()
}

pub fn output(root: &Path, args: &[String]) -> Option<String> {
    if !own_repo(root) {
        return None;
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

pub fn has_commit(root: &Path, hash: &str) -> bool {
    if !own_repo(root) {
        return false;
    }
    let spec = format!("{hash}^{{commit}}");
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &spec])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
