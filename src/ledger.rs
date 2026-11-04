//! The reservation ledger every worktree of a repository shares.
//!
//! The reservations are committed with the store (`reservations/<owner>.toml`, and the legacy
//! `reservations.toml`), so a worktree sees only the blocks that
//! reached its branch, and the store lock is a file inside one checkout. Two loops in two
//! worktrees each took the next block after what their own branch knew, and minted the same
//! rule numbers (a finding). The ledger lives in the git common directory, which every
//! worktree of the repository shares (`git rev-parse --git-common-dir`), under a lock of its
//! own: a block is taken after the highest number the ledger, this checkout, or any other
//! worktree's own `reservations.toml` knows, and every block a checkout writes is also
//! appended here. It needs no network and is never committed; a store outside a git
//! repository has no ledger and behaves as before.

use crate::store::Reservation;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Ledger {
    pub dir: PathBuf,
    pub entries: Vec<Reservation>,
}

pub const FILE: &str = "reservations.toml";

/// `<git common dir>/trail/<store path with / as ->/`, so two stores in one repository keep
/// two ledgers.
pub fn dir_for(root: &Path, store_rel: &str) -> Option<PathBuf> {
    let common = crate::gitinfo::common_dir(root)?;
    let key: String = store_rel
        .trim_matches('/')
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    let key = if key.is_empty() {
        "store".to_string()
    } else {
        key
    };
    Some(common.join("trail").join(key))
}

pub fn load(root: &Path, store_rel: &str) -> Result<Option<Ledger>, String> {
    let Some(dir) = dir_for(root, store_rel) else {
        return Ok(None);
    };
    let mut entries = crate::store::read_reservations(&dir.join(FILE))?;
    entries.extend(peers(root, store_rel));
    Ok(Some(Ledger { dir, entries }))
}

/// The blocks in the other worktrees' own reservation files, committed or not. They cover
/// a block taken before the ledger existed, or by an older trailmark that never wrote it. A
/// file another loop is halfway through writing that does not parse is skipped: the ledger
/// file still holds every block a current trailmark took.
fn peers(root: &Path, store_rel: &str) -> Vec<Reservation> {
    let mut out = Vec::new();
    for path in other_worktrees(root) {
        if let Ok(blocks) = crate::store::read_all_reservations(&path.join(store_rel)) {
            out.extend(blocks.into_iter().map(|mut b| {
                b.worktree = path.display().to_string();
                b
            }));
        }
    }
    out
}

/// The other worktrees of this repository (not this one), committed or not.
fn other_worktrees(root: &Path) -> Vec<PathBuf> {
    let me = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(list) = crate::gitinfo::output(
        root,
        &[
            "worktree".to_string(),
            "list".to_string(),
            "--porcelain".to_string(),
        ],
    ) else {
        return Vec::new();
    };
    list.lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) != me)
        .collect()
}

/// The highest number another worktree's store holds in `folder` (`decisions/0024.md`).
/// A decision, a discussion or an incident takes the next number after every checkout's,
/// not after its own branch's: two worktrees minted the same number for two rulings.
/// Read under the ledger lock (`cli::reserves`), so two adds next door take two numbers.
pub fn peer_high_seq(root: &Path, store_rel: &str, folder: &str) -> u32 {
    other_worktrees(root)
        .iter()
        .filter_map(|wt| std::fs::read_dir(wt.join(store_rel).join(folder)).ok())
        .flat_map(|dir| dir.flatten())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".md")?.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0)
}

fn same(a: &Reservation, b: &Reservation) -> bool {
    a.type_name == b.type_name && a.from == b.from && a.to == b.to && a.for_id == b.for_id
}

/// Append `blocks` that the ledger does not hold yet, each with the worktree that took it.
/// The caller holds the ledger lock.
pub fn append(ledger: &Ledger, root: &Path, blocks: &[Reservation]) -> Result<(), String> {
    let fresh: Vec<&Reservation> = blocks
        .iter()
        .filter(|b| !ledger.entries.iter().any(|e| same(e, b)))
        .collect();
    if fresh.is_empty() {
        return Ok(());
    }
    let path = ledger.dir.join(FILE);
    let mut text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::from(
            "# trailmark's reservation ledger, shared by every worktree of this repository.\n# Never committed; `trailmark reserve` and `trailmark add loop|review` append to it.\n\n",
        ),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let worktree = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for b in fresh {
        text.push_str(&format!(
            "[[reservation]]\ntype = \"{}\"\nfrom = {}\nto = {}\nfor = \"{}\"\ndate = \"{}\"\nworktree = {}\n\n",
            b.type_name,
            b.from,
            b.to,
            b.for_id,
            b.date,
            crate::util::json_escape(&worktree.display().to_string())
        ));
    }
    crate::util::write_atomic(&path, text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Loops and reviews running in the OTHER worktrees of this repository: a loop's record is
/// written in its own worktree and reaches this branch only when it lands, so the Now line
/// of a fresh checkout said "Running: none" while a review ran next door. Each other
/// worktree's reservations name its loops and reviews; the record of each is read there, and
/// it counts when it says `running` there and this store does not have it (a worktree left
/// behind holds the old status of a loop that closed since; this branch has the later word). Returns
/// `(id, worktree directory name)`, sorted. Nothing outside a git repository.
pub fn running_elsewhere(store: &crate::store::Store) -> Vec<(String, String)> {
    let root = &store.root;
    let store_rel = &store.config.store_rel;
    let me = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(list) = crate::gitinfo::output(
        root,
        &[
            "worktree".to_string(),
            "list".to_string(),
            "--porcelain".to_string(),
        ],
    ) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in list.lines() {
        let Some(path) = line.strip_prefix("worktree ") else {
            continue;
        };
        let path = PathBuf::from(path);
        if std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()) == me {
            continue;
        }
        let dir = path.join(store_rel);
        let Ok(blocks) = crate::store::read_all_reservations(&dir) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let mut ids: Vec<String> = blocks.into_iter().map(|b| b.for_id).collect();
        ids.sort();
        ids.dedup();
        for id in ids {
            if out.iter().any(|(seen, _)| seen == &id) {
                continue;
            }
            // Known here: running, it is on the Now line already; anything else, this branch
            // has the later word.
            if store.get(&id).is_some() {
                continue;
            }
            let Some(parsed) = crate::id::Id::parse(&id) else {
                continue;
            };
            let type_name = match parsed.prefix.as_str() {
                "LOOP" => "loop",
                "REV" => "review",
                _ => continue,
            };
            let Some(rel) = crate::id::rel_path(&parsed, type_name, false) else {
                continue;
            };
            let Ok(raw) = std::fs::read(dir.join(&rel)) else {
                continue;
            };
            let Ok(rec) = crate::record::load_record(&rel, raw) else {
                continue;
            };
            if rec.status() == "running" {
                out.push((id, name.clone()));
            }
        }
    }
    out.sort();
    out
}
