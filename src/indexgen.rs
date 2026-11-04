//! Generated indexes. One plan, then either every file is written or none are.
//! tok4 in the header includes the header. Generation fails closed when a file
//! cannot fit its budget.

use crate::id;
use crate::record::{is_tombstone, Record};
use crate::scan;
use crate::store::Store;
use crate::util::{self, tok4};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

/// The store as every checkout has it: without the records under `local/`. Those are
/// gitignored and machine-local, so they are shown, found and checked where they exist, but
/// they never enter a committed index or the store hash. Otherwise a clone, a worktree or
/// another machine would read every index as stale (C08).
pub fn shared_view(store: &Store) -> Cow<'_, Store> {
    if !store.records.iter().any(is_local) {
        return Cow::Borrowed(store);
    }
    let mut shared = store.clone();
    shared.records.retain(|r| !is_local(r));
    shared.invalidate();
    Cow::Owned(shared)
}

fn is_local(rec: &Record) -> bool {
    rec.rel.starts_with("local/")
}

#[derive(Clone, Debug)]
pub struct Planned {
    pub rel: String,
    pub bytes: Vec<u8>,
    pub tok: usize,
    pub budget: Option<usize>,
    pub records: usize,
    /// Set when the file lists fewer records than it covers, and says how many it left out.
    pub folded: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Over {
    pub rel: String,
    pub tok: usize,
    pub budget: usize,
}

#[derive(Debug)]
pub enum IndexFail {
    Over(Vec<Over>),
    Io(String),
}

#[derive(Clone, Debug)]
pub struct LinkRow {
    pub target: String,
    pub cited_by: String,
    pub location: String,
}

pub fn link_rows(store: &Store) -> Vec<LinkRow> {
    let mut rows = Vec::new();
    for rec in &store.records {
        let cited = rec
            .id()
            .map(|i| i.canonical())
            .unwrap_or_else(|| rec.rel.clone());
        for hit in scan::field_hits(store, rec) {
            let loc = match &hit.field {
                Some(name) => format!("field:{name}"),
                None => format!("body:{}", hit.line),
            };
            for id in &hit.ids {
                rows.push(LinkRow {
                    target: id.clone(),
                    cited_by: cited.clone(),
                    location: loc.clone(),
                });
            }
        }
        for hit in scan::body_hits(store, rec) {
            for id in &hit.ids {
                rows.push(LinkRow {
                    target: id.clone(),
                    cited_by: cited.clone(),
                    location: format!("body:{}", hit.line),
                });
            }
        }
    }
    for hit in store.code_hits().iter() {
        for id in &hit.ids {
            rows.push(LinkRow {
                target: id.clone(),
                cited_by: format!("{}:{}", hit.path, hit.line),
                location: "code".into(),
            });
        }
    }
    rows.sort_by(|a, b| {
        a.target
            .cmp(&b.target)
            .then(a.cited_by.cmp(&b.cited_by))
            .then(a.location.cmp(&b.location))
    });
    rows.dedup_by(|a, b| {
        a.target == b.target && a.cited_by == b.cited_by && a.location == b.location
    });
    rows
}

/// Citation counts for ranking. Read from `index/backlinks.tsv` when it is there, so
/// `context` does not rescan the code tree; computed when it is not.
///
/// A committed index (`commit_index = true`) is kept fresh by C08 at check time. A local cache
/// is not: nothing refused a stale one, and after a pull or a landed loop it was trusted until
/// someone knew to run `trailmark index`. So the cache carries a key of what it was built
/// from (`index/.key`, never committed), and a missing or stale cache is rebuilt here, the
/// whole index at once, by whichever command reads it first.
pub fn cites(store: &Store) -> BTreeMap<String, usize> {
    let dir = store.config.store_dir(&store.root);
    if !store.config.commit_index {
        let key = cache_key(store);
        let fresh = std::fs::read_to_string(dir.join(KEY_FILE)).ok().as_deref() == Some(&key);
        // A checkout whose git still tracks the index (a branch cut before ) is not
        // rewritten by a read: its pull or rebase would then refuse 16 modified files.
        if !fresh && tracked_index_files(store) == 0 {
            // A rebuild that cannot write (a read-only checkout, an index over its budget)
            // still answers from the records.
            return match write_keyed(store, &dir, &key) {
                Ok(files) => files
                    .iter()
                    .find(|f| f.rel == "index/backlinks.tsv")
                    .map(|f| counts_of(&String::from_utf8_lossy(&f.bytes)))
                    .unwrap_or_else(|| citation_counts(&link_rows(store))),
                Err(_) => citation_counts(&link_rows(store)),
            };
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("index/backlinks.tsv")) {
        return counts_of(&text);
    }
    citation_counts(&link_rows(store))
}

/// How many files under the store's `index/` git tracks in this checkout (0 outside git).
pub fn tracked_index_files(store: &Store) -> usize {
    let index = format!("{}/index", store.config.store_rel.trim_end_matches('/'));
    crate::gitinfo::output(&store.root, &["ls-files".into(), "--".into(), index])
        .map(|listed| listed.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

fn counts_of(tsv: &str) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for line in tsv.lines() {
        if let Some((target, _)) = line.split_once('\t') {
            *map.entry(target.to_string()).or_insert(0) += 1;
        }
    }
    map
}

/// Where a local cache keeps the key it was built from.
const KEY_FILE: &str = "index/.key";

/// What a local index is built from, as one line: this trailmark's version, `trail.toml`, every
/// record's path and bytes, the reservations, and the code: the checkout's commit, or outside
/// git each code file's path, size and time. The records are in memory already. The code is
/// the commit because walking it was the cost: a large build tree made every `context`
/// slower in the main checkout; a code citation not yet
/// committed counts from the next commit, as it did when only `trailmark index` wrote the cache.
/// Not a fingerprint anything compares across machines: a different key costs one rebuild.
pub fn cache_key(store: &Store) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut h);
    std::fs::read(store.root.join("trail.toml"))
        .unwrap_or_default()
        .hash(&mut h);
    for rec in &store.records {
        rec.rel.hash(&mut h);
        rec.raw.hash(&mut h);
    }
    format!("{:?}", store.reservations).hash(&mut h);
    let head = crate::gitinfo::output(&store.root, &["rev-parse".into(), "HEAD".into()]);
    if let Some(head) = head {
        head.hash(&mut h);
        return format!("{:016x}\n", h.finish());
    }
    let files = scan::code_files(store);
    files.len().hash(&mut h);
    for path in &files {
        path.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(path) {
            meta.len().hash(&mut h);
            if let Ok(t) = meta.modified() {
                t.hash(&mut h);
            }
        }
    }
    format!("{:016x}\n", h.finish())
}

pub fn citation_counts(rows: &[LinkRow]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for row in rows {
        *map.entry(row.target.clone()).or_insert(0) += 1;
    }
    map
}

pub fn plan(store: &Store) -> Result<Vec<Planned>, Vec<Over>> {
    let view = shared_view(store);
    let store = view.as_ref();
    let rows = link_rows(store);
    let cites = citation_counts(&rows);
    let mut files = Vec::new();
    let mut overs = Vec::new();
    let consider = |files: &mut Vec<Planned>,
                    overs: &mut Vec<Over>,
                    rel: String,
                    body: String,
                    budget: usize,
                    records: usize| {
        match finish(&kind_of(&rel), &attrs_of(&rel), &body, budget, records) {
            Ok(text) => files.push(Planned {
                folded: folded_note(&text),
                rel,
                tok: tok4(&text),
                budget: Some(budget),
                records,
                bytes: text.into_bytes(),
            }),
            Err((tok, budget)) => overs.push(Over { rel, tok, budget }),
        }
    };

    let (core, core_n) = core_markdown(store, &cites);
    consider(
        &mut files,
        &mut overs,
        "index/core.md".into(),
        core,
        store.config.budgets.core,
        core_n,
    );

    let mut areas: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for rec in active_rules(store) {
        for area in rec.areas() {
            areas.entry(area).or_default().push(rec);
        }
    }
    for (area, mut rules) in areas {
        rules.sort_by_key(|a| rule_sort(a));
        let rel = format!("index/rules-{area}.md");
        match fit_area(store, &area, &rules, &cites) {
            Ok((body, n)) => consider(
                &mut files,
                &mut overs,
                rel,
                body,
                store.config.budgets.rules_area_index,
                n,
            ),
            Err(tok) => overs.push(Over {
                rel,
                tok,
                budget: store.config.budgets.rules_area_index,
            }),
        }
    }

    let hot = hot_rules(store, &cites);
    if let Some((body, n)) = fit_hot(store, &hot, &cites) {
        consider(
            &mut files,
            &mut overs,
            "index/rules-hot.md".into(),
            body,
            store.config.budgets.hot_rules,
            n,
        );
    } else {
        let empty = "# Most-cited rules\n\n";
        let tok = finish("rules-hot", "", empty, store.config.budgets.hot_rules, 0)
            .err()
            .map(|(t, _)| t)
            .unwrap_or(0);
        overs.push(Over {
            rel: "index/rules-hot.md".into(),
            tok,
            budget: store.config.budgets.hot_rules,
        });
    }

    if let Some((body, n)) = fit_open(store) {
        consider(
            &mut files,
            &mut overs,
            "index/open.md".into(),
            body,
            store.config.budgets.open_index,
            n,
        );
    } else {
        overs.push(Over {
            rel: "index/open.md".into(),
            tok: 0,
            budget: store.config.budgets.open_index,
        });
    }
    if let Some((body, n)) = fit_decisions(store) {
        consider(
            &mut files,
            &mut overs,
            "index/decisions.md".into(),
            body,
            store.config.budgets.decisions_index,
            n,
        );
    } else {
        overs.push(Over {
            rel: "index/decisions.md".into(),
            tok: 0,
            budget: store.config.budgets.decisions_index,
        });
    }
    if let Some((body, n)) = fit_loops(store) {
        consider(
            &mut files,
            &mut overs,
            "index/loops.md".into(),
            body,
            store.config.budgets.loops_index,
            n,
        );
    } else {
        overs.push(Over {
            rel: "index/loops.md".into(),
            tok: 0,
            budget: store.config.budgets.loops_index,
        });
    }
    if let Some((body, n)) = fit_deploys(store) {
        consider(
            &mut files,
            &mut overs,
            "index/deploys.md".into(),
            body,
            store.config.budgets.deploys_index,
            n,
        );
    } else {
        overs.push(Over {
            rel: "index/deploys.md".into(),
            tok: 0,
            budget: store.config.budgets.deploys_index,
        });
    }
    match fit_items(store) {
        Ok((body, n)) => {
            consider(
                &mut files,
                &mut overs,
                "index/items.md".into(),
                body,
                store.config.budgets.items_index,
                n,
            );
        }
        Err(tok) => overs.push(Over {
            rel: "index/items.md".into(),
            tok,
            budget: store.config.budgets.items_index,
        }),
    }

    let (start, start_n) = start_markdown(store);
    consider(
        &mut files,
        &mut overs,
        "index/start.md".into(),
        start,
        store.config.budgets.start_index,
        start_n,
    );

    files.push(tsv("index/ids.tsv", &ids_tsv(store)));
    files.push(tsv("index/backlinks.tsv", &links_tsv(&rows)));
    files.push(tsv("index/code-refs.tsv", &code_tsv(store)));

    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    if overs.is_empty() {
        Ok(files)
    } else {
        Err(overs)
    }
}

/// `index/.gitignore` when the indexes are not committed: the directory ignores everything
/// in it, itself included.
pub const IGNORE_ALL: &[u8] =
    b"# trailmark's indexes are a local cache (commit_index = false); `trailmark index` rebuilds them\n*\n";

pub fn write_to(store: &Store, store_dir: &Path) -> Result<Vec<Planned>, IndexFail> {
    let key = (!store.config.commit_index).then(|| cache_key(store));
    write_with(store, store_dir, key.as_deref())
}

fn write_keyed(store: &Store, store_dir: &Path, key: &str) -> Result<Vec<Planned>, IndexFail> {
    write_with(store, store_dir, Some(key))
}

fn write_with(
    store: &Store,
    store_dir: &Path,
    key: Option<&str>,
) -> Result<Vec<Planned>, IndexFail> {
    let files = plan(store).map_err(IndexFail::Over)?;
    let index = store_dir.join("index");
    std::fs::create_dir_all(&index).map_err(|e| IndexFail::Io(e.to_string()))?;
    let mut keep = std::collections::BTreeSet::new();
    // A local cache ignores itself, so a repository needs no `.gitignore` line for it and
    // `git add <store>` never stages it.
    if !store.config.commit_index {
        util::write_atomic(&index.join(".gitignore"), IGNORE_ALL)
            .map_err(|e| IndexFail::Io(format!("index/.gitignore: {e}")))?;
        keep.insert("index/.gitignore".to_string());
        // The key goes last (below), so a rebuild cut short leaves a stale key, never a
        // fresh key over half-written files.
        keep.insert(KEY_FILE.to_string());
    }
    for file in &files {
        let path = store_dir.join(&file.rel);
        util::write_atomic(&path, &file.bytes)
            .map_err(|e| IndexFail::Io(format!("{}: {e}", file.rel)))?;
        keep.insert(file.rel.clone());
    }
    if index.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&index) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                let rel = format!("index/{name}");
                if path.is_file() && !keep.contains(&rel) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
    if let (false, Some(key)) = (store.config.commit_index, key) {
        util::write_atomic(&store_dir.join(KEY_FILE), key.as_bytes())
            .map_err(|e| IndexFail::Io(format!("{KEY_FILE}: {e}")))?;
    }
    Ok(files)
}

/// A folded file ends its list with a count line such as "496 more rules (trailmark find …)".
fn folded_note(text: &str) -> Option<String> {
    text.lines()
        .find(|l| {
            let first = l.split_whitespace().next().unwrap_or("");
            !first.is_empty()
                && first.chars().all(|c| c.is_ascii_digit())
                && (l.contains("(trailmark find") || l.contains("more rules"))
        })
        .map(|l| l.to_string())
}

fn tsv(rel: &str, body: &str) -> Planned {
    Planned {
        folded: None,
        rel: rel.into(),
        tok: tok4(body),
        budget: None,
        records: body.lines().count(),
        bytes: body.as_bytes().to_vec(),
    }
}

/// The header names what the file is and its size, never the store: a store hash in every
/// header changed all of them on every `trailmark add`, so two worktrees that each added a record
/// conflicted on every index file when they landed. `check` compares the bytes.
fn finish(
    kind: &str,
    attrs: &str,
    body: &str,
    budget: usize,
    records: usize,
) -> Result<String, (usize, usize)> {
    let mut tok = 0usize;
    for _ in 0..8 {
        let header = format!(
            "<!-- trailmark:generated kind={kind}{attrs} records={records} tok4={tok} budget={budget} — do not edit; run `trailmark index` -->\n"
        );
        let full = format!("{header}{body}");
        let n = tok4(&full);
        if n == tok {
            if n > budget {
                return Err((n, budget));
            }
            return Ok(full);
        }
        tok = n;
    }
    Err((tok, budget))
}

fn kind_of(rel: &str) -> String {
    match rel {
        "index/core.md" => "core".into(),
        "index/rules-hot.md" => "rules-hot".into(),
        "index/open.md" => "open".into(),
        "index/decisions.md" => "decisions".into(),
        "index/loops.md" => "loops".into(),
        "index/deploys.md" => "deploys".into(),
        "index/items.md" => "items".into(),
        "index/start.md" => "start".into(),
        other => other
            .trim_start_matches("index/")
            .trim_end_matches(".md")
            .into(),
    }
}

fn attrs_of(rel: &str) -> String {
    rel.strip_prefix("index/rules-")
        .and_then(|r| r.strip_suffix(".md"))
        .filter(|a| *a != "hot")
        .map(|a| format!(" area={a}"))
        .unwrap_or_default()
}

fn active_rules(store: &Store) -> Vec<&Record> {
    store
        .records
        .iter()
        .filter(|r| r.type_name() == "rule" && !is_tombstone(r.status()))
        .collect()
}

fn rule_sort(rec: &Record) -> (u32, String) {
    let key = rec.id().map(|i| i.key).unwrap_or_default();
    (id::rule_number(&key).unwrap_or(0), key)
}

pub fn rule_line(rec: &Record) -> String {
    format!("{}  {}", rec.id_str(), rule_label(rec))
}

fn rule_label(rec: &Record) -> String {
    match rec.fm.get_str("short") {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => util::cut_words(rec.title(), 72),
    }
}

fn core_markdown(store: &Store, cites: &BTreeMap<String, usize>) -> (String, usize) {
    let mut body = String::new();
    let mut n = 0usize;
    match &store.config.name {
        Some(name) if !name.is_empty() => body.push_str(&format!("# Start here — {name}\n\n")),
        _ => body.push_str("# Start here\n\n"),
    }
    body.push_str(
        "Rules, decisions and reports are records with ids. Never read a whole document. Ask:\n",
    );
    body.push_str("  trailmark context \"<your task>\"   ·   trailmark show <id>   ·   trailmark find <words>   ·   trailmark add rule|open …   ·   trailmark check\n");
    body.push_str("Cite ids in code comments and commit messages.\n\n");

    let scheme = id::scheme();
    let pinned_id = format!("{}-1", scheme.charter);
    if let Some(wa1) = store.get(&pinned_id) {
        body.push_str(&format!("## Non-negotiables ({pinned_id})\n"));
        for bullet in bullets(&wa1.body) {
            body.push_str(&format!("- {}\n", util::cut_words(&bullet, 200)));
        }
        body.push('\n');
        n += 1;
    }

    let step_prefixes: Vec<String> = if scheme.charter_also.is_empty() {
        vec![scheme.charter.clone()]
    } else {
        scheme.charter_also.clone()
    };
    let charter_is_steps = step_prefixes.iter().any(|p| p == &scheme.charter);
    let mut steps: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| {
            r.type_name() == "charter"
                && r.id()
                    .map(|i| {
                        step_prefixes.iter().any(|p| p == &i.prefix)
                            && i.key.chars().all(|c| c.is_ascii_digit())
                            && !(charter_is_steps && i.key == "1")
                    })
                    .unwrap_or(false)
        })
        .filter(|r| !is_tombstone(r.status()))
        .collect();
    steps.sort_by_key(|r| r.id().and_then(|i| i.key.parse::<u32>().ok()).unwrap_or(0));
    if !steps.is_empty() {
        body.push_str("## The loop\n");
        for step in &steps {
            body.push_str(&format!("{}  {}\n", step.id_str(), step_line(step)));
            n += 1;
        }
        body.push('\n');
    }

    let mut pins: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "decision" && r.pinned() && r.status() == "active")
        .collect();
    pins.sort_by_key(|r| r.id_str());
    if !pins.is_empty() {
        body.push_str("## Product principles\n");
        for dec in &pins {
            body.push_str(&format!("{}  {}\n", dec.id_str(), one_line(dec.title())));
            n += 1;
        }
        body.push('\n');
    }

    body.push_str("## Now\n");
    body.push_str(&now_line(store));
    body.push_str("\n\n");

    let mut hot = hot_rules(store, cites);
    hot.sort_by(|a, b| {
        cites
            .get(&b.id_str())
            .unwrap_or(&0)
            .cmp(cites.get(&a.id_str()).unwrap_or(&0))
            .then(a.id_str().cmp(&b.id_str()))
    });
    // A fixed short list, not "whatever room is left": the core is the same for every task,
    // and every line here is a line the task's own rules cannot use (shows five).
    let chosen: Vec<&Record> = hot
        .into_iter()
        .take(store.config.budgets.core_hot_rules)
        .collect();
    if !chosen.is_empty() {
        body.push_str("## Most-cited rules (index/rules-hot.md has more)\n");
        for c in &chosen {
            body.push_str(&rule_line(c));
            body.push('\n');
            n += 1;
        }
    }
    (body, n.max(1))
}

/// The top-level bullets of a body, each with its continuation lines joined.
fn bullets(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("- ") {
            out.push(rest.trim().to_string());
        } else if line.starts_with("  ") && !line.trim().is_empty() {
            if let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    out.into_iter()
        .map(|b| b.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// A loop step's line: its bold title and the start of its text, so the line says
/// what the step is.
fn step_line(step: &Record) -> String {
    let first = step
        .body
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    let text: String = first
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
        .replace("**", "");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        one_line(step.title())
    } else {
        util::cut_words(&flat, 110)
    }
}

/// The few lines a harness loads at the start of every session (the memory index
/// rewritten to ≤ 400 tok4): how to ask for context, and the pinned facts.
fn start_markdown(store: &Store) -> (String, usize) {
    let mut body = String::from("# Start here\n\n");
    body.push_str("Before any task run `trailmark context \"<task>\"` and work from that bundle. Never read a whole document, a generated index or the store directory.\n");
    body.push_str("Look things up with `trailmark show <id>` and `trailmark find <words>`. Record a lesson with `trailmark add rule --for <LOOP-ID>`. Run `trailmark check` before you finish.\n");
    let mut pinned: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "fact" && r.pinned() && r.status() == "active")
        .collect();
    pinned.sort_by_key(|r| r.id_str());
    if !pinned.is_empty() {
        body.push_str("\n## Pinned facts\n");
        for r in &pinned {
            body.push_str(&format!("{}  {}\n", r.id_str(), one_line(r.title())));
        }
    }
    (body, pinned.len().max(1))
}

fn one_line(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    util::cut_words(&flat, 120)
}

fn now_line(store: &Store) -> String {
    let mut lives: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "deploy" && r.status() == "live")
        .collect();
    lives.sort_by(|a, b| {
        let aw = if a.fm.get_str("target") == Some("web") {
            0
        } else {
            1
        };
        let bw = if b.fm.get_str("target") == Some("web") {
            0
        } else {
            1
        };
        // The newest live deploy first: by time, then by date, then by deploy number.
        aw.cmp(&bw)
            .then(deploy_time(b).cmp(&deploy_time(a)))
            .then(deploy_number(b).cmp(&deploy_number(a)))
    });
    let prod = if let Some(dep) = lives.first() {
        let mut facts = vec![dep.date().to_string()];
        facts.extend(deploy_facts(dep));
        format!("{} ({})", dep.id_str(), facts.join(", "))
    } else {
        "none".into()
    };
    let running = running_list(store);
    let mut queued: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "item" && r.status() == "queued")
        .collect();
    queued.sort_by(|a, b| {
        order_of(a)
            .cmp(&order_of(b))
            .then(a.id_str().cmp(&b.id_str()))
    });
    let queued = queued
        .first()
        .map(|r| r.id_str())
        .unwrap_or_else(|| "none".into());
    let owed = store
        .records
        .iter()
        .filter(|r| {
            r.type_name() == "open"
                && r.status() == "open"
                && r.fm.get_str("owner") == Some("author")
        })
        .count();
    let owed_word = if owed == 1 { "open item" } else { "open items" };
    format!("Production {prod}. Running: {running}. Queued first: {queued}. Owed by the author: {owed} {owed_word} (trailmark find --type open --status open).")
}

/// What is running in this store: loops and reviews by id, then the items whose status is
/// `running`. A bare "running" with no title used to print as "Running: none". Items are
/// named when there are a few; past that, counted with the command that lists them, so a
/// long queue of old entries does not fill the line.
pub fn running_list(store: &Store) -> String {
    const ITEMS_NAMED: usize = 3;
    let ids = |t: &str| {
        let mut v: Vec<String> = store
            .records
            .iter()
            .filter(|r| r.type_name() == t && r.status() == "running")
            .map(|r| r.id_str())
            .collect();
        v.sort();
        v
    };
    let mut parts: Vec<String> = ids("loop");
    parts.extend(ids("review"));
    let items = ids("item");
    if items.len() > ITEMS_NAMED {
        parts.push(format!(
            "{} items marked running (trailmark find --type item --status running)",
            items.len()
        ));
    } else {
        parts.extend(items);
    }
    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(", ")
    }
}

/// A deploy's commit, migrations and rollback, each only when the record has it.
/// A record with only a date must not print an empty field after that date.
fn deploy_facts(dep: &Record) -> Vec<String> {
    let mut facts = Vec::new();
    for key in ["commit", "migrations"] {
        if let Some(v) = dep.fm.get_str(key).filter(|v| !v.trim().is_empty()) {
            facts.push(one_line(v));
        }
    }
    if let Some(roll) = dep
        .fm
        .strings("rollback")
        .into_iter()
        .next()
        .filter(|r| !r.trim().is_empty())
    {
        facts.push(format!("rollback: {}", one_line(&roll)));
    }
    facts
}

fn order_of(rec: &Record) -> i64 {
    match rec.fm.get("order") {
        Some(crate::frontmatter::Value::Int(n)) => *n,
        Some(crate::frontmatter::Value::String(s)) => s.parse().unwrap_or(i64::MAX),
        _ => i64::MAX,
    }
}

fn hot_rules<'a>(store: &'a Store, cites: &BTreeMap<String, usize>) -> Vec<&'a Record> {
    let mut rules: Vec<&Record> = active_rules(store)
        .into_iter()
        .filter(|r| r.pinned() || cites.get(&r.id_str()).copied().unwrap_or(0) > 0)
        .collect();
    rules.sort_by(|a, b| {
        let ca = cites.get(&a.id_str()).copied().unwrap_or(0);
        let cb = cites.get(&b.id_str()).copied().unwrap_or(0);
        cb.cmp(&ca).then(a.id_str().cmp(&b.id_str()))
    });
    rules
}

fn fit_hot(
    store: &Store,
    rules: &[&Record],
    cites: &BTreeMap<String, usize>,
) -> Option<(String, usize)> {
    let budget = store.config.budgets.hot_rules;
    let mut keep: Vec<&Record> = rules.to_vec();
    loop {
        let mut body = String::from("# Most-cited rules\n\n");
        for rec in &keep {
            let n = cites.get(&rec.id_str()).copied().unwrap_or(0);
            body.push_str(&format!("{} (cited {n})\n", rule_line(rec)));
        }
        if finish("rules-hot", "", &body, budget, keep.len()).is_ok() {
            return Some((body, keep.len()));
        }
        if keep.is_empty() {
            return None;
        }
        if let Some(i) = keep.iter().rposition(|r| !r.pinned()) {
            keep.remove(i);
        } else {
            keep.pop();
        }
    }
}

fn fit_open(store: &Store) -> Option<(String, usize)> {
    let budget = store.config.budgets.open_index;
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "open" && r.status() == "open")
        .collect();
    items.sort_by_key(|a| open_key(a));
    let full = open_body(&items, &[]);
    if finish("open", "", &full, budget, items.len()).is_ok() {
        return Some((full, items.len()));
    }
    let (listed, rest): (Vec<&Record>, Vec<&Record>) = items.into_iter().partition(|r| {
        r.fm.get_str("owner") == Some("author")
            || matches!(r.fm.get_str("severity"), Some("high" | "medium"))
    });
    let body = open_body(&listed, &rest);
    if finish("open", "", &body, budget, listed.len()).is_ok() {
        Some((body, listed.len()))
    } else {
        None
    }
}

fn open_key(rec: &Record) -> (u8, u8, String, String) {
    let owner = match rec.fm.get_str("owner").unwrap_or("") {
        "author" => 0,
        "orchestrator" => 1,
        "next-loop" => 2,
        _ => 3,
    };
    let sev = match rec.fm.get_str("severity").unwrap_or("") {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    };
    (owner, sev, rec.date().to_string(), rec.id_str())
}

fn open_body(listed: &[&Record], folded: &[&Record]) -> String {
    let mut body = String::from("# Open items\n\n");
    for rec in listed {
        let sev = rec.fm.get_str("severity").unwrap_or("");
        let owner = rec.fm.get_str("owner").unwrap_or("");
        body.push_str(&format!(
            "{}  {sev}  {owner}  {}\n",
            rec.id_str(),
            one_line(rec.title())
        ));
    }
    if !folded.is_empty() {
        let mut by_area: BTreeMap<String, usize> = BTreeMap::new();
        for rec in folded {
            let areas = rec.areas();
            let key = if areas.is_empty() {
                "unset".into()
            } else {
                areas.join(",")
            };
            *by_area.entry(key).or_insert(0) += 1;
        }
        for (area, n) in by_area {
            body.push_str(&format!(
                "{n} low open items in {area} (trailmark find --type open --area {area})\n"
            ));
        }
    }
    body
}

fn fit_decisions(store: &Store) -> Option<(String, usize)> {
    let budget = store.config.budgets.decisions_index;
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "decision" && r.status() == "active")
        .collect();
    items.sort_by(|a, b| {
        b.pinned()
            .cmp(&a.pinned())
            .then(scope_of(a).cmp(&scope_of(b)))
            .then(a.id_str().cmp(&b.id_str()))
    });
    let mut dropped: Vec<&Record> = Vec::new();
    loop {
        let body = decision_body(&items, &dropped);
        if finish("decisions", "", &body, budget, items.len()).is_ok() {
            return Some((body, items.len()));
        }
        let pos = items.iter().rposition(|r| !r.pinned())?;
        // Oldest unpinned: among unpinned, smallest date.
        let oldest = items
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.pinned())
            .min_by(|(_, a), (_, b)| a.date().cmp(b.date()).then(a.id_str().cmp(&b.id_str())))
            .map(|(i, _)| i)
            .unwrap_or(pos);
        dropped.push(items.remove(oldest));
        if items.is_empty() && !dropped.is_empty() {
            let body = decision_body(&items, &dropped);
            if finish("decisions", "", &body, budget, 0).is_ok() {
                return Some((body, 0));
            }
            return None;
        }
    }
}

fn scope_of(rec: &Record) -> String {
    rec.fm.get_str("scope").unwrap_or("").to_string()
}

fn decision_body(items: &[&Record], dropped: &[&Record]) -> String {
    let mut body = String::from("# Decisions\n\n");
    for rec in items {
        let scope = rec.fm.get_str("scope").unwrap_or("");
        body.push_str(&format!(
            "{}  {scope}  {}\n",
            rec.id_str(),
            one_line(rec.title())
        ));
    }
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for rec in dropped {
        let scope = if scope_of(rec).is_empty() {
            "unset".into()
        } else {
            scope_of(rec)
        };
        *by.entry(scope).or_insert(0) += 1;
    }
    for (scope, n) in by {
        body.push_str(&format!(
            "{n} older {scope} decisions (trailmark find --type decision)\n"
        ));
    }
    body
}

fn fit_loops(store: &Store) -> Option<(String, usize)> {
    let budget = store.config.budgets.loops_index;
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "loop")
        .collect();
    items.sort_by(|a, b| b.date().cmp(a.date()).then(a.id_str().cmp(&b.id_str())));
    let mut n = items.len().min(40);
    loop {
        let body = loop_body(&items[..n], items.len() - n);
        if finish("loops", "", &body, budget, n).is_ok() {
            return Some((body, n));
        }
        if n == 0 {
            return None;
        }
        n -= 1;
    }
}

fn loop_body(items: &[&Record], rest: usize) -> String {
    let mut body = String::from("# Loops\n\n");
    for rec in items {
        let platforms = rec.fm.strings("platform").join(",");
        let review = rec.fm.get_str("review").unwrap_or("");
        body.push_str(&format!(
            "{}  {}  {}  {platforms}  {}  → {review}\n",
            rec.id_str(),
            rec.date(),
            rec.status(),
            one_line(rec.title())
        ));
    }
    if rest > 0 {
        body.push_str(&format!(
            "{rest} older loops (trailmark find --type loop)\n"
        ));
    }
    body
}

fn fit_deploys(store: &Store) -> Option<(String, usize)> {
    let budget = store.config.budgets.deploys_index;
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "deploy")
        .collect();
    items.sort_by(|a, b| {
        deploy_time(b)
            .cmp(&deploy_time(a))
            .then(deploy_number(b).cmp(&deploy_number(a)))
    });
    let infra_n = items
        .iter()
        .filter(|r| r.fm.get_str("target") == Some("infra"))
        .count();
    let mut web = 8usize;
    let mut mobile = 4usize;
    let mut drop_infra = 0usize;
    loop {
        let body = deploy_body(&items, web, mobile, drop_infra);
        let n = body.lines().filter(|l| l.starts_with("DEP-")).count();
        if finish("deploys", "", &body, budget, n).is_ok() {
            return Some((body, n));
        }
        if drop_infra < infra_n {
            drop_infra += 1;
        } else if web > 0 {
            web -= 1;
        } else if mobile > 0 {
            mobile -= 1;
        } else {
            return None;
        }
    }
}

fn deploy_number(rec: &Record) -> Vec<u64> {
    rec.id()
        .map(|i| i.key.split('.').filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_default()
}

fn deploy_time(rec: &Record) -> String {
    rec.fm.get_str("time").unwrap_or(rec.date()).to_string()
}

fn deploy_body(
    items: &[&Record],
    web_cap: usize,
    mobile_cap: usize,
    drop_oldest_infra: usize,
) -> String {
    let infra_at: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, r)| r.fm.get_str("target") == Some("infra"))
        .map(|(i, _)| i)
        .collect();
    let skip_infra: std::collections::BTreeSet<usize> = infra_at
        .iter()
        .rev()
        .take(drop_oldest_infra)
        .copied()
        .collect();
    let mut web = 0usize;
    let mut mobile = 0usize;
    let mut body = String::from("# Deploys\n\n");
    let mut shown = 0usize;
    for (i, rec) in items.iter().enumerate() {
        let target = rec.fm.get_str("target").unwrap_or("");
        if target == "mobile" {
            if mobile >= mobile_cap {
                continue;
            }
            mobile += 1;
        } else if target == "infra" {
            if skip_infra.contains(&i) {
                continue;
            }
        } else if web >= web_cap {
            continue;
        } else {
            web += 1;
        }
        let mut line = format!("{}  {}  {}", rec.id_str(), deploy_time(rec), rec.status());
        for fact in deploy_facts(rec) {
            line.push_str("  ");
            line.push_str(&fact);
        }
        if let Some(backup) = rec.fm.get_str("backup").filter(|b| !b.trim().is_empty()) {
            line.push_str(&format!("  backup {backup}"));
        }
        body.push_str(&line);
        body.push('\n');
        shown += 1;
    }
    let rest = items.len().saturating_sub(shown);
    if rest > 0 {
        body.push_str(&format!(
            "{rest} older deploys (trailmark find --type deploy)\n"
        ));
    }
    body
}

fn fit_area(
    store: &Store,
    area: &str,
    rules: &[&Record],
    cites: &BTreeMap<String, usize>,
) -> Result<(String, usize), usize> {
    let budget = store.config.budgets.rules_area_index;
    let rel = format!("index/rules-{area}.md");
    let full = area_text(area, rules, 0);
    match finish(&kind_of(&rel), &attrs_of(&rel), &full, budget, rules.len()) {
        Ok(_) => return Ok((full, rules.len())),
        // The default: an area index that cannot list every rule fails, and the fix is
        // in the data (split the area, add `short:` lines). Folding is a host's explicit choice.
        Err((tok, _)) if !store.config.budgets.fold_rules_area => return Err(tok),
        Err(_) => {}
    }
    let mut ranked = rules.to_vec();
    ranked.sort_by(|a, b| {
        cites
            .get(&b.id_str())
            .unwrap_or(&0)
            .cmp(cites.get(&a.id_str()).unwrap_or(&0))
            .then(a.id_str().cmp(&b.id_str()))
    });
    let mut last = 0usize;
    for keep in (0..ranked.len()).rev() {
        let body = area_text(area, &ranked[..keep], ranked.len() - keep);
        match finish(&kind_of(&rel), &attrs_of(&rel), &body, budget, keep) {
            Ok(_) => return Ok((body, keep)),
            Err((tok, _)) => last = tok,
        }
    }
    Err(last)
}

fn area_text(area: &str, shown: &[&Record], rest: usize) -> String {
    let mut body = format!("# Rules — {area}\n\n");
    for rec in shown {
        body.push_str(&rule_line(rec));
        body.push('\n');
    }
    if rest > 0 {
        body.push_str(&format!(
            "{rest} more rules (trailmark find --type rule --area {area})\n"
        ));
    }
    body
}

fn fit_items(store: &Store) -> Result<(String, usize), usize> {
    let budget = store.config.budgets.items_index;
    let stages: &[&[&str]] = &[
        &[],
        &["done", "dropped"],
        &["done", "dropped", "parked", "waiting", "built"],
        &["done", "dropped", "parked", "waiting", "built", "queued"],
    ];
    let mut last = 0usize;
    for fold in stages {
        let body = items_body(store, fold);
        let listed = store
            .records
            .iter()
            .filter(|r| r.type_name() == "item" && !fold.contains(&r.status()))
            .count();
        match finish("items", "", &body, budget, listed) {
            Ok(_) => return Ok((body, listed)),
            Err((tok, _)) => last = tok,
        }
    }
    Err(last)
}

fn items_body(store: &Store, fold: &[&str]) -> String {
    let order = [
        "running", "queued", "waiting", "built", "parked", "done", "dropped",
    ];
    let mut body = String::from("# Items\n\n");
    let mut seen = std::collections::BTreeSet::new();
    for status in order {
        seen.insert(status);
        body.push_str(&item_section(store, status, fold.contains(&status)));
    }
    let mut extra: Vec<&str> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "item")
        .map(|r| r.status())
        .filter(|s| !seen.contains(s) && !s.is_empty())
        .collect();
    extra.sort();
    extra.dedup();
    for status in extra {
        body.push_str(&item_section(store, status, fold.contains(&status)));
    }
    body
}

fn item_section(store: &Store, status: &str, fold_done: bool) -> String {
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "item" && r.status() == status)
        .collect();
    if items.is_empty() {
        return String::new();
    }
    items.sort_by(|a, b| {
        order_of(a)
            .cmp(&order_of(b))
            .then(a.id_str().cmp(&b.id_str()))
    });
    let mut body = format!("## {}\n\n", status_heading(status));
    if fold_done {
        body.push_str(&format!(
            "{} {status} items (trailmark find --type item --status {status})\n\n",
            items.len()
        ));
        return body;
    }
    for rec in items {
        let ord = match order_of(rec) {
            n if n == i64::MAX => String::new(),
            n if status == "queued" => format!(" #{n}"),
            _ => String::new(),
        };
        body.push_str(&format!(
            "{}  {status}{ord}  {}\n",
            rec.id_str(),
            one_line(rec.title())
        ));
    }
    body.push('\n');
    body
}

fn status_heading(status: &str) -> String {
    let mut chars = status.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn ids_tsv(store: &Store) -> String {
    let mut lines = Vec::new();
    for rec in &store.records {
        let id = rec.id_str();
        if id.is_empty() {
            continue;
        }
        let title = tsv_cell(&one_line_n(rec.title(), 100));
        let areas = rec.areas().join(",");
        lines.push(format!(
            "{id}\t{}\t{}\t{}\t{areas}\t{}\t{title}",
            rec.type_name(),
            rec.status(),
            rec.date(),
            rec.rel
        ));
    }
    lines.sort();
    join_lines(&lines)
}

fn one_line_n(s: &str, n: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(n).collect()
}

fn links_tsv(rows: &[LinkRow]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|r| format!("{}\t{}\t{}", r.target, r.cited_by, r.location))
        .collect();
    join_lines(&lines)
}

fn code_tsv(store: &Store) -> String {
    let mut lines = Vec::new();
    for hit in store.code_hits().iter() {
        for id in &hit.ids {
            lines.push(format!(
                "{}:{}\t{id}\t{}",
                hit.path,
                hit.line,
                tsv_cell(&hit.written)
            ));
        }
    }
    lines.sort();
    join_lines(&lines)
}

fn tsv_cell(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

/// The core index, including its header. Fails when the core itself is over budget.
pub fn core_text(store: &Store, cites: &BTreeMap<String, usize>) -> Result<String, Over> {
    let view = shared_view(store);
    let store = view.as_ref();
    let (body, n) = core_markdown(store, cites);
    finish("core", "", &body, store.config.budgets.core, n.max(1)).map_err(|(tok, budget)| Over {
        rel: "index/core.md".into(),
        tok,
        budget,
    })
}
