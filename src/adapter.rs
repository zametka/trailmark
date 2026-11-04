//! Harness adapters. Each adapter is a folder with an `install.toml` that says which files
//! go where. The core reads that manifest; it does not know any harness's folder names.

use crate::config::{self, Config};
use crate::util;
use std::path::{Path, PathBuf};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/adapters.rs"));
}

/// Where the adapters' files come from: the copy built into the binary, or a folder
/// named by `TRAIL_ADAPTERS`.
enum Source {
    Embedded,
    Dir(PathBuf),
}

impl Source {
    fn current() -> Source {
        match std::env::var_os("TRAIL_ADAPTERS") {
            Some(dir) if !dir.is_empty() => Source::Dir(PathBuf::from(dir)),
            _ => Source::Embedded,
        }
    }

    /// `rel` is `<adapter>/<path inside it>`.
    fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        match self {
            Source::Dir(dir) => std::fs::read(dir.join(rel)),
            Source::Embedded => embedded::EMBEDDED
                .iter()
                .find(|(path, _)| *path == rel)
                .map(|(_, bytes)| bytes.to_vec())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "not in this build")
                }),
        }
    }

    fn describe(&self, rel: &str) -> String {
        match self {
            Source::Dir(dir) => dir.join(rel).display().to_string(),
            Source::Embedded => format!("{rel} (built in)"),
        }
    }
}

/// The adapters this binary carries, for `trailmark adapter` without a name.
pub fn names() -> Vec<String> {
    let mut out: Vec<String> = embedded::EMBEDDED
        .iter()
        .filter_map(|(p, _)| p.strip_suffix("/install.toml"))
        .filter(|p| !p.contains('/'))
        .map(|p| p.to_string())
        .collect();
    out.sort();
    out
}

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

struct Plan {
    message: String,
    copies: Vec<(String, String)>,
    never_whole: Option<String>,
    settings: Option<(String, String)>,
    memory_table: Option<String>,
}

pub fn install(root: &Path, name: &str, dry_run: bool, force: bool) -> Outcome {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return fail(2, format!("unknown adapter {name}"));
    }
    let source = Source::current();
    let plan = match read_plan(&source, name) {
        Ok(p) => p,
        Err(e) => return fail(2, format!("unknown adapter {name}: {e}")),
    };
    let config = config::load_config(root).ok();
    let mut stdout = String::from("adapter files:\n");
    let mut stderr = String::new();
    let mut differ = false;
    let mut writes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for (from, to) in &plan.copies {
        let bytes = match source.read(from) {
            Ok(b) => b,
            Err(e) => return fail(4, format!("{}: {e}", source.describe(from))),
        };
        stdout.push_str(&format!("  {to}\n"));
        let target = root.join(to);
        if let Some(diff) = differs(&target, &bytes) {
            differ = true;
            stderr.push_str(&format!("{to} exists and differs:\n{diff}"));
        }
        writes.push((target, bytes));
    }
    if let Some(to) = &plan.never_whole {
        stdout.push_str(&format!("  {to} (the never-whole list from trail.toml)\n"));
        let bytes = never_whole_text(config.as_ref()).into_bytes();
        let target = root.join(to);
        if let Some(diff) = differs(&target, &bytes) {
            differ = true;
            stderr.push_str(&format!("{to} exists and differs:\n{diff}"));
        }
        writes.push((target, bytes));
    }
    if dry_run {
        if plan.settings.is_some() {
            stdout.push_str(
                "Dry run. A missing settings file is created. An existing one is left alone and the block to merge is printed.\n",
            );
        } else {
            stdout.push_str("Dry run.\n");
        }
        if differ {
            stdout.push_str("Some files differ (see above); rerun with --force to replace them.\n");
        }
        stdout.push_str(&plan.message);
        return Outcome {
            code: 0,
            stdout,
            stderr,
        };
    }
    if differ && !force {
        stderr.push_str("Nothing was written. Rerun with --force to replace these files.\n");
        return Outcome {
            code: 7,
            stdout,
            stderr,
        };
    }
    for (target, bytes) in &writes {
        if let Err(e) = util::write_atomic(target, bytes) {
            return fail(4, format!("{}: {e}", target.display()));
        }
    }
    if let Some((from, to)) = &plan.settings {
        // Never replace an existing settings file. When it is absent, write it, so a
        // fresh install turns the guard on. When it exists, print the block to merge.
        let snippet = source
            .read(from)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let target = root.join(to);
        if target.exists() {
            stderr.push_str(&format!(
                "{to} exists and is not replaced. Merge this block into it by hand:\n{snippet}"
            ));
        } else if let Err(e) = util::write_atomic(&target, snippet.as_bytes()) {
            return fail(4, e.to_string());
        } else {
            stdout.push_str(&format!("wrote {to}\n"));
        }
    }
    if let (Some(table), true) = (&plan.memory_table, force) {
        match install_memory(root, config.as_ref(), table) {
            Ok(line) => stdout.push_str(&line),
            Err(e) => return fail(4, e),
        }
    }
    stdout.push_str(&plan.message);
    Outcome {
        code: 0,
        stdout,
        stderr,
    }
}

/// Replace the harness's memory index with the store's generated `index/start.md`. The old
/// file is kept beside it as `<name>.pre-trailmark.md`, once; nothing is deleted.
fn install_memory(root: &Path, config: Option<&Config>, table: &str) -> Result<String, String> {
    let Some(config) = config else {
        return Ok(String::new());
    };
    let Some(target) = config.adapter_memory.get(table) else {
        return Ok(String::new());
    };
    let target = config::expand_home(target);
    let start = config.store_dir(root).join("index/start.md");
    let bytes = std::fs::read(&start)
        .map_err(|_| format!("{} is missing; run trailmark index first", start.display()))?;
    if target.is_file() {
        let stem = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("MEMORY");
        let keep = target.with_file_name(format!("{stem}.pre-trailmark.md"));
        if !keep.exists() {
            std::fs::copy(&target, &keep).map_err(|e| format!("{}: {e}", keep.display()))?;
        }
    }
    util::write_atomic(&target, &bytes).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(format!(
        "wrote {} from index/start.md (the old one is kept as .pre-trailmark.md)\n",
        target.display()
    ))
}

fn read_plan(source: &Source, name: &str) -> Result<Plan, String> {
    let text = source
        .read(&format!("{name}/install.toml"))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|_| "no install.toml".to_string())?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("install.toml: {e}"))?;
    if let Some(t) = v.as_table() {
        for key in t.keys() {
            if !matches!(
                key.as_str(),
                "message" | "copy" | "never_whole" | "settings" | "memory"
            ) {
                return Err(format!("install.toml: unknown key '{key}'"));
            }
        }
    }
    let rel = |s: &str| -> Result<String, String> {
        if s.starts_with('/') || s.split('/').any(|p| p == "..") {
            Err(format!(
                "install.toml: '{s}' must stay inside the repository"
            ))
        } else {
            Ok(s.to_string())
        }
    };
    let mut copies = Vec::new();
    for item in v
        .get("copy")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
    {
        let from = item
            .get("from")
            .and_then(|x| x.as_str())
            .ok_or("copy needs from")?;
        let to = item
            .get("to")
            .and_then(|x| x.as_str())
            .ok_or("copy needs to")?;
        copies.push((format!("{name}/{}", rel(from)?), rel(to)?));
    }
    let never_whole = match v
        .get("never_whole")
        .and_then(|x| x.get("to"))
        .and_then(|x| x.as_str())
    {
        Some(s) => Some(rel(s)?),
        None => None,
    };
    let settings = match v.get("settings") {
        Some(s) => {
            let from = s
                .get("from")
                .and_then(|x| x.as_str())
                .ok_or("settings needs from")?;
            let to = s
                .get("to")
                .and_then(|x| x.as_str())
                .ok_or("settings needs to")?;
            Some((format!("{name}/{}", rel(from)?), rel(to)?))
        }
        None => None,
    };
    let memory_table = v
        .get("memory")
        .and_then(|m| m.get("config_table"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let mut message = v
        .get("message")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if !message.is_empty() && !message.ends_with('\n') {
        message.push('\n');
    }
    Ok(Plan {
        message,
        copies,
        never_whole,
        settings,
        memory_table,
    })
}

/// A short line diff of an existing file against what would be written, or None when the
/// file is absent or identical.
fn differs(target: &Path, new: &[u8]) -> Option<String> {
    let old = std::fs::read(target).ok()?;
    if old == new {
        return None;
    }
    let old = String::from_utf8_lossy(&old);
    let new = String::from_utf8_lossy(new);
    Some(line_diff(&old, &new))
}

pub fn line_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // Longest common subsequence; adapter files are small.
    let mut l = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            l[i][j] = if a[i] == b[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = String::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || l[i][j + 1] >= l[i + 1][j]) {
            out.push_str(&format!("  + {}\n", b[j]));
            j += 1;
        } else {
            out.push_str(&format!("  - {}\n", a[i]));
            i += 1;
        }
    }
    out
}

fn never_whole_text(config: Option<&Config>) -> String {
    let mut lines = config.map(|c| c.never_whole.clone()).unwrap_or_default();
    lines.sort();
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

fn fail(code: i32, message: impl Into<String>) -> Outcome {
    let mut stderr = message.into();
    if !stderr.ends_with('\n') {
        stderr.push('\n');
    }
    Outcome {
        code,
        stdout: String::new(),
        stderr,
    }
}
