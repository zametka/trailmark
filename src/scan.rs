use crate::id::{Id, Resolved};
use crate::record::{self, is_sentinel, Record};
use crate::store::Store;
use crate::util::{self, glob_match};
use regex::Regex;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub written: String,
    pub ids: Vec<String>,
    pub ambiguous: bool,
    pub part: Option<String>,
    pub field: Option<String>,
    /// Matched only by a `context_only` alias pattern (`#85`), not by the id grammar.
    pub informal: bool,
}

/// The citation pattern for the prefixes loaded from `[ids]`. A project that sets
/// `rule = "LESSON"` is scanned for `LESSON-1`, and not for a prefix it did not choose.
/// Case-sensitive: a citation uses the canonical uppercase prefix. `open-sesame`,
/// `rev-parse` and `Dec-31` are words, not ids. Command-line input stays case-insensitive
/// through `Id::parse`. A fact's key is a hyphenated slug, so that prefix takes hyphens;
/// every other key stops at the first one.
pub fn id_regex() -> Regex {
    thread_local! {
        static CACHED: RefCell<Option<(String, Regex)>> = const { RefCell::new(None) };
    }
    let scheme = crate::id::scheme();
    let key = scheme.prefixes().join("\n");
    CACHED.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((cached, re)) = slot.as_ref() {
            if cached == &key {
                return re.clone();
            }
        }
        let re = Regex::new(&citation_pattern(&scheme)).expect("id pattern");
        *slot = Some((key, re.clone()));
        re
    })
}

fn citation_pattern(scheme: &crate::id::IdScheme) -> String {
    let fact = regex::escape(&scheme.fact);
    let mut others: Vec<String> = scheme
        .prefixes()
        .into_iter()
        .filter(|p| p != &scheme.fact)
        .map(|p| regex::escape(&p))
        .collect();
    others.sort();
    others.dedup();
    let alt = others.join("|");
    format!(r"\b(?:{fact}-[a-z0-9]+(?:-[a-z0-9]+)*|(?:{alt})-[A-Za-z0-9.]+)(?:#[A-Za-z0-9-]+)?\b")
}

pub fn code_files(store: &Store) -> Vec<PathBuf> {
    if store.config.code_include.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    walk(&store.root, &store.root, store, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, root: &Path, store: &Store, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // A symlinked directory (`pkg/web ->../web`) is the same files again: every
        // citation in it would be counted, and reported, twice.
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') && name != "." && name != ".github" {
            continue;
        }
        if matches!(name, "node_modules" | "target" | "dist" | ".git" | ".trail") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if store
            .config
            .code_exclude
            .iter()
            .any(|g| glob_match(g, &rel))
        {
            continue;
        }
        if path.is_dir() {
            // A directory itself is not a file. Still skip if an exclude matched the dir.
            walk(&path, root, store, out);
            continue;
        }
        if store
            .config
            .code_include
            .iter()
            .any(|g| glob_match(g, &rel))
        {
            out.push(path);
        }
    }
}

pub fn scan_file(
    store: &Store,
    path: &Path,
    text: &str,
    allow_context_only: bool,
    mask_code: bool,
) -> Vec<Hit> {
    let rel = path
        .strip_prefix(&store.root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    scan_text(store, &rel, text, 1, allow_context_only, mask_code, None)
}

pub fn scan_text(
    store: &Store,
    path: &str,
    text: &str,
    line_base: usize,
    allow_context_only: bool,
    mask_code: bool,
    field: Option<&str>,
) -> Vec<Hit> {
    let mask = if mask_code {
        code_mask(text)
    } else {
        vec![false; text.len()]
    };
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    for m in id_regex().find_iter(text) {
        spans.push((m.start(), m.end(), false));
    }
    for pat in &store.config.alias_patterns {
        if pat.context_only && !allow_context_only {
            continue;
        }
        for m in pat.re.find_iter(text) {
            spans.push((m.start(), m.end(), pat.context_only));
        }
    }
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut taken: Vec<(usize, usize, bool)> = Vec::new();
    let mut cursor = 0;
    for (s, e, informal) in spans {
        if s < cursor || e <= s {
            continue;
        }
        if mask.get(s).copied().unwrap_or(false) {
            continue;
        }
        taken.push((s, e, informal));
        cursor = e;
    }
    let mut hits = Vec::new();
    for (s, e, informal) in taken {
        let written = text[s..e].to_string();
        let line = line_base + text[..s].bytes().filter(|b| *b == b'\n').count();
        let token = crate::store::id_part(&written).to_string();
        let part = written
            .strip_prefix(token.as_str())
            .and_then(|rest| rest.strip_prefix('#'))
            .map(|p| p.to_string());
        let resolved = store.resolve_token(&token, allow_context_only);
        let (ids, ambiguous) = match resolved {
            Resolved::One(id) => (vec![id.canonical()], false),
            Resolved::Range(ids) => (ids.into_iter().map(|i| i.canonical()).collect(), false),
            Resolved::Ambiguous(ids) => (ids.into_iter().map(|i| i.canonical()).collect(), true),
            Resolved::Unknown | Resolved::Malformed => (Vec::new(), false),
        };
        hits.push(Hit {
            informal,
            path: path.to_string(),
            line,
            written,
            ids,
            ambiguous,
            part,
            field: field.map(|s| s.to_string()),
        });
    }
    hits
}

pub fn body_hits(store: &Store, rec: &Record) -> Vec<Hit> {
    scan_text(store, &rec.rel, &rec.body, rec.body_line, true, true, None)
}

pub fn field_hits(store: &Store, rec: &Record) -> Vec<Hit> {
    const ID_FIELDS: &[&str] = &[
        "source",
        "links",
        "supersedes",
        "superseded_by",
        "narrows",
        "narrowed_by",
        "either",
        "follows",
        "missed",
        "earned_by",
        "decided_in",
        "items",
        "decisions",
        "loops",
        "deploy",
        "item",
        "learned",
        "opened",
        "closes",
        "review",
        "reviews",
        "for_deploy",
        "opened_by",
        "closed_by",
        "carries",
        "previous",
        "owed",
        "found_by",
        "earned",
        "deploys",
        "waiting_on",
        "decided_by",
        "owner",
    ];
    let mut hits = Vec::new();
    for key in ID_FIELDS {
        for value in rec.fm.strings(key) {
            if is_sentinel(&value) {
                continue;
            }
            if is_path_token(&value) {
                continue;
            }
            let line = rec.field_line(key);
            let resolved = store.resolve_token(&value, true);
            let (ids, ambiguous, part) = match &resolved {
                Resolved::One(id) => (vec![id.canonical()], false, part_of(&value)),
                Resolved::Range(ids) => (ids.iter().map(|i| i.canonical()).collect(), false, None),
                Resolved::Ambiguous(ids) => {
                    (ids.iter().map(|i| i.canonical()).collect(), true, None)
                }
                _ => {
                    // Not an id and not an alias. decided_by/owner may be a free word
                    // we already treated sentinels. A leftover word is not a link.
                    if Id::parse(value.split('#').next().unwrap_or(&value)).is_none()
                        && !value.starts_with('§')
                    {
                        continue;
                    }
                    (Vec::new(), false, part_of(&value))
                }
            };
            hits.push(Hit {
                informal: false,
                path: rec.rel.clone(),
                line,
                written: value,
                ids,
                ambiguous,
                part,
                field: Some((*key).to_string()),
            });
        }
    }
    hits
}

fn part_of(value: &str) -> Option<String> {
    value.split_once('#').map(|(_, p)| p.to_string())
}

pub fn is_path_token(s: &str) -> bool {
    let base = s.split('#').next().unwrap_or(s);
    base.contains('/') || base.ends_with(".md") || base.ends_with(".txt") || base.ends_with(".sh")
}

pub fn path_targets(rec: &Record) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for key in ["links", "enforced_in", "release_notes"] {
        for value in rec.fm.strings(key) {
            if is_path_token(&value) {
                out.push((rec.rel.clone(), rec.field_line(key), value));
            }
        }
    }
    let path_re = Regex::new(r"(?:^|[\s(])([A-Za-z0-9_./-]+\.md)(#[A-Za-z0-9_-]+)?").unwrap();
    for (i, line) in rec.body.lines().enumerate() {
        for caps in path_re.captures_iter(line) {
            let mut token = caps.get(1).unwrap().as_str().to_string();
            if let Some(anchor) = caps.get(2) {
                token.push_str(anchor.as_str());
            }
            out.push((rec.rel.clone(), rec.body_line + i, token));
        }
    }
    out
}

pub fn path_exists(root: &Path, token: &str, beside: Option<&str>) -> Result<(), String> {
    let (path, anchor) = token
        .split_once('#')
        .map(|(p, a)| (p, Some(a)))
        .unwrap_or((token, None));
    // A migrated body links relative to the document it came from (`priorities.md` from
    // `docs/inbox-…`), so that directory is tried after the repository root.
    let mut full = root.join(path);
    if !full.exists() {
        if let Some(dir) = beside.and_then(|s| Path::new(s).parent()) {
            let near = root.join(dir).join(path);
            if near.exists() {
                full = near;
            }
        }
    }
    if !full.exists() {
        return Err(format!("path {path} does not exist"));
    }
    if let Some(anchor) = anchor {
        if path.ends_with(".md") {
            if let Ok(text) = std::fs::read_to_string(&full) {
                let slugs: Vec<String> = text
                    .lines()
                    .filter(|l| l.starts_with('#'))
                    .map(util::heading_slug)
                    .collect();
                if !slugs.iter().any(|s| s == anchor) {
                    return Err(format!("heading #{anchor} is missing in {path}"));
                }
            }
        }
    }
    Ok(())
}

fn code_mask(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut fence = false;
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        let line = &text[start..i];
        if line.trim_start().starts_with("```") {
            mask[start..i].fill(true);
            fence = !fence;
        } else if fence {
            mask[start..i].fill(true);
        } else {
            let lb = line.as_bytes();
            let mut j = 0;
            while j < lb.len() {
                if lb[j] == b'`' {
                    let mut k = j + 1;
                    while k < lb.len() && lb[k] != b'`' {
                        k += 1;
                    }
                    if k < lb.len() {
                        mask[start + j..=start + k].fill(true);
                        j = k + 1;
                        continue;
                    }
                }
                j += 1;
            }
        }
        if i < bytes.len() {
            i += 1;
        }
    }
    mask
}

pub fn part_exists(rec: &Record, part: &str) -> bool {
    record::find_part(&record::parts_of(rec), part).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{push_scheme, IdScheme};

    #[test]
    fn a_custom_prefix_is_what_code_is_scanned_for() {
        let _guard = push_scheme(IdScheme {
            rule: "LESSON".into(),
            ..IdScheme::default()
        });
        let re = id_regex();
        assert!(re.is_match("// see LESSON-1 before shipping"));
        assert!(re.is_match("DEC-0001"));
        assert!(!re.is_match("// see RULE-1"));
        assert!(!re.is_match("// see GUIDE-2.1"));
        assert!(!re.is_match("// see STEP-2"));
        let fact = id_regex();
        assert!(fact.is_match("FACT-oven-log"));
    }
}
