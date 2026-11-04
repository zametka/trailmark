//! Command dispatch. `run` is the public entry.

use crate::check::{self, CheckOpts};
use crate::config;
use crate::context::{self, Request};
use crate::frontmatter::Value;
use crate::id::{Id, Resolved};
use crate::indexgen::{self, IndexFail, Planned};
use crate::issue::{self, Severity};
use crate::mutate::{self, AddRequest};
use crate::record::{self, is_tombstone, Record};
use crate::search::{self, bm25};
use crate::store::Store;
use crate::util::{self, tok4};
use crate::Outcome;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Cmd {
    positionals: Vec<String>,
    values: BTreeMap<String, Vec<String>>,
    bools: BTreeSet<String>,
}

const COMMANDS: &str = "init, show, find, context, terms, add, close, set, link, tag, retire, figure, reserve, index, check, why, measure, adapter";

/// `help`, `--help` and `--version` answer without a `trail.toml`: a reader who has not set
/// one up yet is the reader who asks (a cold-read test hit exit 4 on `trailmark --help`).
fn help_without_store(args: &[String]) -> Option<Outcome> {
    let first = args.first().map(|s| s.as_str()).unwrap_or("");
    if args.iter().any(|a| a == "--version" || a == "-V") || first == "version" {
        return Some(ok(format!("trailmark {}\n", env!("CARGO_PKG_VERSION"))));
    }
    let asked =
        first.is_empty() || first == "help" || args.iter().any(|a| a == "--help" || a == "-h");
    if !asked {
        return None;
    }
    let topic = if first == "help" {
        args.get(1).map(|s| s.as_str()).unwrap_or("")
    } else if first.starts_with('-') {
        ""
    } else {
        first
    };
    // `trailmark add <type> --help` / `trailmark help add <type>`: the type's required fields and
    // parts, with an example (a cold read: "help lists flag names only").
    if topic == "add" {
        let at = if first == "help" { 2 } else { 1 };
        let kind = args.get(at).filter(|a| !a.starts_with('-'));
        let text = match kind.and_then(|k| crate::typehelp::find(k)) {
            Some(t) => crate::typehelp::render(t),
            None => {
                let mut text = crate::typehelp::overview(flags_of("add").unwrap_or(&[]));
                if let Some(k) = kind {
                    text = format!("unknown type {k}\n\n{text}");
                }
                text
            }
        };
        return Some(ok(text));
    }
    if topic == "set" {
        return Some(ok(SET_HELP.to_string()));
    }
    if topic == "adapter" {
        return Some(ok(
            "trailmark adapter install <name> [--dry-run] [--force]\n\
             With no name, lists the adapters this build carries.\n\
             --force replaces the adapter's copied files. For the hook adapter, that is the skill,\n\
             both hook scripts, and never-whole.txt. When that adapter's memory_md is set, it also\n\
             replaces that index and keeps the old file once as <name>.pre-trailmark.md.\n\
             A missing settings file is created, with or without --force. An existing settings file is\n\
             never replaced: the block to merge is printed on stderr and the command exits 0.\n\
             Without --force, a copied file that differs is left in place and the command exits 7,\n\
             having written nothing.\n"
                .to_string(),
        ));
    }
    if topic == "init" {
        return Some(ok(
            "trailmark init [--root <dir>]\n\
             Writes trail.toml, docs/records/, and AGENTS.md when that file is absent.\n\
             Ids default to RULE-1, DEC-0001, OPEN-0001. Refuses to replace an existing trail.toml.\n"
                .to_string(),
        ));
    }
    let text = match flags_of(topic) {
        Some(flags) if !flags.is_empty() => format!(
            "trailmark {topic}: flags --{}\nSee README.md for the command sequences.\n",
            flags.join(", --")
        ),
        Some(_) => format!("trailmark {topic}: no flags\n"),
        None => format!(
            "trailmark <command> [args] [--root <dir>] [--format json]\nCommands: {COMMANDS}\n\
             `trailmark help <command>` lists its flags. See README.md for the command sequences.\n"
        ),
    };
    Some(if first.is_empty() {
        Outcome {
            code: 2,
            stdout: String::new(),
            stderr: text,
        }
    } else {
        ok(text)
    })
}

const SET_HELP: &str = "trailmark set <ITEM-X|OPEN-N> status=<value> --why \"<one line>\" [--by <id|author>]
  An item:      status=queued|running|built|waiting|parked|done|dropped; status=waiting needs waiting_on=<author|id>;
                order=<n> reorders the queue; --by LOOP-X adds the loop to its loops,
                --by DEP-N with status=done sets its deploy.
  An open item: status=open|closed|wontfix; closed needs --by (closed_by, closed date);
                wontfix appends the --why line as its ## Why not part.
  Writes status_note: \"<date> <old> → <new>: <why> (by <id>)\". Nothing else in the body changes.
  A note alone (no status=, or the status it has): \"<date> note: <why>\", the earlier note kept in
                status_history; for a correction to the last why. Nothing else changes.
  A loop or review is closed by `trailmark close`; a rule, decision or fact leaves by `trailmark retire`.
Example:
  trailmark set ITEM-B1 status=done --why \"the till build shipped\" --by LOOP-B1
  trailmark set OPEN-0001 status=closed --why \"the scale was replaced\" --by LOOP-B1
  trailmark set OPEN-0001 --why \"correction: the spare log is still on the pass\" --by REV-B1
";

/// A loop or review named by its short key (`B1`) is the record `LOOP-B1` / `REV-B1`;
/// the spec allows the short form wherever the type is already known.
fn with_prefix(prefix: &str, id: &str) -> String {
    let upper = id.to_ascii_uppercase();
    if upper.starts_with(&format!("{prefix}-")) {
        return canon_or(id);
    }
    static KEY: OnceLock<Regex> = OnceLock::new();
    let key = KEY.get_or_init(|| Regex::new(r"^[A-Za-z]{1,4}\d+[a-z]?(\.\d+)?$").unwrap());
    if Id::parse(id).is_some() || !key.is_match(id) {
        return canon_or(id);
    }
    canon_or(&format!("{prefix}-{id}"))
}

pub fn dispatch(args: &[String], stdin: &str) -> Outcome {
    if let Some(out) = help_without_store(args) {
        return out;
    }
    let (root, cmd) = match split(args) {
        Ok(v) => v,
        Err(e) if e.contains("trail.toml") || e.contains("requires") => {
            return Outcome {
                code: 4,
                stdout: String::new(),
                stderr: format!("{e}\n"),
            };
        }
        Err(e) => return usage(e),
    };
    let name = cmd.positionals.first().map(|s| s.as_str()).unwrap_or("");
    if name.is_empty() || name == "help" || cmd.bools.contains("help") {
        return usage(format!("trailmark <command>. Commands: {COMMANDS}"));
    }
    let rest = Cmd {
        positionals: cmd.positionals[1..].to_vec(),
        values: cmd.values,
        bools: cmd.bools,
    };
    if let Some(allowed) = flags_of(name) {
        for flag in rest.values.keys().chain(rest.bools.iter()) {
            if !allowed.contains(&flag.as_str()) && !GLOBAL_FLAGS.contains(&flag.as_str()) {
                let hint = util::suggest(flag, allowed.iter().copied())
                    .map(|s| format!(" (did you mean --{s}?)"))
                    .unwrap_or_default();
                return usage(format!("{name}: unknown flag --{flag}{hint}"));
            }
        }
    }
    // Commands that write hold the store lock for their whole read-change-write.
    let _lock = if writes(name, &rest) {
        let dir = match config::load_config(&root) {
            Ok(c) => c.store_dir(&root),
            Err(e) => {
                return Outcome {
                    code: 4,
                    stdout: String::new(),
                    stderr: format!("{e}\n"),
                }
            }
        };
        if dir.is_dir() {
            match util::Lock::take(&dir) {
                Ok(lock) => Some(lock),
                Err(e) => {
                    return Outcome {
                        code: 7,
                        stdout: String::new(),
                        stderr: format!("{e}\n"),
                    }
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    // A command that takes a block of numbers also holds the shared ledger's lock, after the
    // store's: worktrees of one repository then never take the same block (ledger.rs).
    let _ledger_lock = if reserves(name, &rest) {
        match ledger_lock(&root) {
            Ok(lock) => lock,
            Err(e) => {
                return Outcome {
                    code: 7,
                    stdout: String::new(),
                    stderr: format!("{e}\n"),
                }
            }
        }
    } else {
        None
    };
    match name {
        "init" => init_cmd(&root),
        "show" => show(&root, &rest),
        "find" => find(&root, &rest),
        "context" => context_cmd(&root, &rest),
        "terms" => terms_cmd(&root, &rest),
        "add" => add(&root, &rest, stdin),
        "close" => close(&root, &rest),
        "link" => link(&root, &rest),
        "retire" => retire(&root, &rest),
        "set" => set(&root, &rest),
        "figure" => figure(&root, &rest),
        "tag" => tag(&root, &rest),
        "reserve" => reserve(&root, &rest),
        "index" => index(&root, &rest),
        "check" => check_cmd(&root, &rest),
        "why" => why(&root, &rest),
        "measure" => measure(&root, &rest),
        "adapter" => adapter_cmd(&root, &rest),
        other => usage(format!("unknown command {other}")),
    }
}

fn split(args: &[String]) -> Result<(PathBuf, Cmd), String> {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut bools = BTreeSet::new();
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(name) = arg.strip_prefix("--") {
            if name.is_empty() {
                return Err("empty flag".into());
            }
            if is_bool(name) {
                bools.insert(name.to_string());
                i += 1;
                continue;
            }
            let value = args
                .get(i + 1)
                .cloned()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            values.entry(name.to_string()).or_default().push(value);
            i += 2;
            continue;
        }
        positionals.push(arg.clone());
        i += 1;
    }
    let command = positionals.first().map(|s| s.as_str()).unwrap_or("");
    let root = if let Some(path) = values.get("root").and_then(|v| v.first()) {
        PathBuf::from(path)
    } else if let Ok(path) = std::env::var("TRAIL_ROOT") {
        PathBuf::from(path)
    } else if command == "init" {
        PathBuf::from(".")
    } else {
        config::find_root(Path::new("."))?
    };
    Ok((
        root,
        Cmd {
            positionals,
            values,
            bools,
        },
    ))
}

const GLOBAL_FLAGS: &[&str] = &["root", "format", "help"];

/// The flags each command takes. An unknown flag is a usage error (exit 2), never ignored.
fn flags_of(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "init" => &[],
        "show" => &["part", "fields", "links", "budget"],
        "find" => &[
            "type", "area", "status", "since", "part", "limit", "snippet",
        ],
        "context" => &[
            "loop", "review", "item", "core", "budget", "area", "explain", "terms",
        ],
        "terms" => &["all", "budget"],
        "add" => &[
            "id",
            "for",
            "field",
            "title",
            "body",
            "body-file",
            "part",
            "areas",
            "item",
            "platform",
            "kind",
            "severity",
            "owner",
            "verdict",
            "figure",
        ],
        "close" => &["gates", "review", "verdict", "part", "field"],
        "link" => &["rel", "remove"],
        "retire" => &["why", "by"],
        "set" => &["why", "by"],
        "figure" => &["add", "drop"],
        "tag" => &["add", "drop"],
        "reserve" => &["for"],
        "index" => &["check"],
        "check" => &["changed", "history", "strict"],
        "why" => &[],
        "measure" => &["tasks", "tokenizer"],
        "adapter" => &["dry-run", "force"],
        _ => return None,
    })
}

fn reserves(name: &str, cmd: &Cmd) -> bool {
    name == "reserve"
        || (name == "add"
            && matches!(
                cmd.positionals.first().map(|s| s.as_str()),
                Some("loop" | "review" | "decision" | "discussion" | "incident")
            ))
}

fn ledger_lock(root: &Path) -> Result<Option<util::Lock>, String> {
    let Ok(config) = config::load_config(root) else {
        return Ok(None);
    };
    let Some(dir) = crate::ledger::dir_for(root, &config.store_rel) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    util::Lock::take(&dir).map(Some)
}

fn writes(name: &str, cmd: &Cmd) -> bool {
    match name {
        "add" | "close" | "link" | "retire" | "set" | "reserve" | "figure" | "tag" => true,
        "index" => !cmd.bools.contains("check"),
        _ => false,
    }
}

fn is_bool(name: &str) -> bool {
    matches!(
        name,
        "links"
            | "snippet"
            | "strict"
            | "history"
            | "changed"
            | "check"
            | "verify"
            | "force"
            | "dry-run"
            | "core"
            | "explain"
            | "remove"
            | "with-new"
            | "all"
            | "help"
    )
}

fn init_cmd(root: &Path) -> Outcome {
    if root.join("trail.toml").is_file() {
        return Outcome {
            code: 7,
            stdout: String::new(),
            stderr: format!("{} already has a trail.toml\n", root.display()),
        };
    }
    let raw = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let name: String = raw
        .chars()
        .map(|c| if c == '"' || c == '\n' { ' ' } else { c })
        .collect();
    let toml = format!(
        "name = \"{name}\"\n\
         requires = \">=0.1.0\"\n\
         store = \"docs/records\"\n\
         cutover = false\n\
         frozen = false\n\
         never_whole = [\"docs/records/\"]\n\
         \n\
         [areas]\n\
         app = {{ keywords = [\"screen\", \"page\"], paths = [\"src/**\"] }}\n\
         process = {{ keywords = [\"review\", \"loop\"], paths = [] }}\n\
         \n\
         [code]\n\
         include = [\"src/**\"]\n\
         exclude = [\"**/node_modules/**\", \"**/target/**\", \"**/dist/**\"]\n\
         \n\
         [reserve]\n\
         rule = 10\n\
         open = 10\n"
    );
    let store = root.join("docs/records");
    if let Err(e) = std::fs::create_dir_all(&store) {
        return Outcome {
            code: 4,
            stdout: String::new(),
            stderr: format!("{}: {e}\n", store.display()),
        };
    }
    if let Err(e) = std::fs::write(root.join("trail.toml"), toml) {
        return Outcome {
            code: 4,
            stdout: String::new(),
            stderr: format!("trail.toml: {e}\n"),
        };
    }
    let agents = include_str!("../adapters/generic/agents-block.md");
    let agents_path = root.join("AGENTS.md");
    let mut stderr = String::new();
    if agents_path.exists() {
        stderr.push_str(
            "AGENTS.md already exists and was not changed. Put this block at the top of it:\n\n",
        );
        stderr.push_str(agents);
        stderr.push('\n');
    } else if let Err(e) = std::fs::write(&agents_path, agents) {
        return Outcome {
            code: 4,
            stdout: String::new(),
            stderr: format!("AGENTS.md: {e}\n"),
        };
    }
    let ignore = root.join(".gitignore");
    if !ignore.exists() {
        let _ = std::fs::write(ignore, ".trail/\ndocs/records/local/\n");
    }
    Outcome {
        code: 0,
        stdout: "Wrote trail.toml, docs/records/, and AGENTS.md (when it was absent).\n\
                 Ids: RULE-1, DEC-0001, OPEN-0001, LOOP-<KEY>, REV-<KEY>, ITEM-<KEY>, FACT-<slug>.\n\
                 Next (each --part reads a file you wrote; trailmark add <type> --help lists them):\n  \
                 trailmark add loop P1 --title \"First slice\" --item none --platform docs --part brief=brief.md\n  \
                 trailmark add rule --for P1 --title \"Names stay stable\" --areas app --part incident=incident.md --part rule=rule.md\n  \
                 trailmark check\n  \
                 trailmark adapter install <name>\n"
            .into(),
        stderr,
    }
}

fn show(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if cmd.positionals.is_empty() {
        return usage("show needs an id");
    }
    let part = cmd.opt("part");
    let fields = cmd.opt("fields");
    let links = cmd.bools.contains("links");
    let rows = if links {
        indexgen::link_rows(&store)
    } else {
        Vec::new()
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut code = 0;
    for token in &cmd.positionals {
        match store.resolve_token(token, true) {
            Resolved::Malformed => {
                // A bare key (a cold reader: `show CAP1`) names the records that have it.
                let have: Vec<String> = ["LOOP", "REV", "ITEM"]
                    .iter()
                    .map(|p| with_prefix(p, token))
                    .filter(|id| store.get(id).is_some())
                    .collect();
                let hint = if have.is_empty() {
                    String::new()
                } else {
                    format!(" (did you mean {}?)", have.join(", "))
                };
                return usage(format!("malformed id {token}{hint}"));
            }
            Resolved::Unknown => {
                code = 1;
                stderr.push_str(&missing(&store, token));
            }
            Resolved::Ambiguous(ids) => {
                code = 1;
                stderr.push_str(&format!(
                    "{token} is ambiguous: {}\n",
                    ids.iter()
                        .map(|i| i.canonical())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Resolved::One(id) => match render_one(
                &store,
                &id,
                part.as_deref(),
                fields.as_deref(),
                links,
                &rows,
            ) {
                Ok(text) => stdout.push_str(&text),
                Err(e) => {
                    code = 1;
                    stderr.push_str(&e);
                }
            },
            Resolved::Range(ids) => {
                if ids.is_empty() {
                    code = 1;
                    stderr.push_str(&format!("range resolves to nothing ({token})\n"));
                }
                for id in ids {
                    match render_one(
                        &store,
                        &id,
                        part.as_deref(),
                        fields.as_deref(),
                        links,
                        &rows,
                    ) {
                        Ok(text) => stdout.push_str(&text),
                        Err(e) => {
                            code = 1;
                            stderr.push_str(&e);
                        }
                    }
                }
            }
        }
    }
    if let Some(budget) = cmd.opt("budget").and_then(|s| s.parse::<usize>().ok()) {
        stdout = cut_at(
            &stdout,
            budget,
            &format!("trailmark show {}", cmd.positionals.join(" ")),
        );
    }
    if cmd.format() == "json" {
        stdout = show_json(&store, &cmd.positionals);
    }
    Outcome {
        code,
        stdout,
        stderr,
    }
}

fn render_one(
    store: &Store,
    id: &Id,
    part: Option<&str>,
    fields: Option<&str>,
    links: bool,
    rows: &[indexgen::LinkRow],
) -> Result<String, String> {
    let Some(rec) = store.get(&id.canonical()) else {
        return Err(missing(store, &id.canonical()));
    };
    let mut text = if let Some(name) = part {
        let parts = record::parts_of(rec);
        let Some(found) = record::find_part(&parts, name) else {
            return Err(format!("{} has no part {name}\n", rec.id_str()));
        };
        let mut block = format!(
            "{}#{} — {}\n{}",
            rec.id_str(),
            found.name,
            rec.title(),
            found.text
        );
        if !block.ends_with('\n') {
            block.push('\n');
        }
        block
    } else if let Some(list) = fields {
        let mut block = String::new();
        for key in list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if let Some(value) = rec.fm.get(key) {
                block.push_str(&format!("{key}: {}\n", value_text(value)));
            }
        }
        block
    } else {
        let mut raw = String::from_utf8_lossy(&rec.raw).into_owned();
        if !raw.ends_with('\n') {
            raw.push('\n');
        }
        raw
    };
    if part.is_none() && fields.is_none() {
        text.push_str(&crate::relate::trailer(store, rec));
    }
    if links {
        text.push_str(&link_lines(rec, rows));
    }
    Ok(text)
}

fn link_lines(rec: &Record, rows: &[indexgen::LinkRow]) -> String {
    let mut out = String::new();
    let id = rec.id_str();
    for row in rows
        .iter()
        .filter(|r| r.cited_by == id || r.cited_by.starts_with(&format!("{id}:")))
    {
        out.push_str(&format!("→ cites {} ({})\n", row.target, row.location));
    }
    let mut incoming: Vec<String> = rows
        .iter()
        .filter(|r| r.target == id)
        .map(|r| r.cited_by.clone())
        .collect();
    incoming.sort();
    incoming.dedup();
    if !incoming.is_empty() {
        out.push_str(&format!("← cited by {}\n", incoming.join(", ")));
    }
    out
}

fn find(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // No words with a filter lists what the filter holds, newest first (`find --type item
    // --status running`, which and the Now line name); no words and no filter is still
    // a usage error.
    let listing = cmd.positionals.is_empty();
    if listing
        && !["type", "area", "status", "since"]
            .iter()
            .any(|f| cmd.opt(f).is_some())
    {
        return usage("find needs words, or a filter (--type, --area, --status, --since)");
    }
    let query = search::query_terms(&cmd.positionals.join(" "));
    let want_part = cmd.opt("part");
    let want_type = cmd.opt("type");
    let want_area = cmd.opt("area");
    let want_status = cmd.opt("status");
    let since = cmd.opt("since");
    let limit: usize = cmd.opt("limit").and_then(|s| s.parse().ok()).unwrap_or(20);
    let snippet = cmd.bools.contains("snippet");
    let show_tombs = matches!(
        want_status.as_deref(),
        Some("retired" | "superseded" | "all")
    );
    let mut docs = Vec::new();
    let mut chosen: Vec<&Record> = Vec::new();
    for rec in &store.records {
        if let Some(t) = &want_type {
            if rec.type_name() != t {
                continue;
            }
        }
        if let Some(area) = &want_area {
            if !rec.areas().iter().any(|a| a == area) {
                continue;
            }
        }
        if let Some(status) = &want_status {
            if status != "all" && rec.status() != status {
                continue;
            }
        } else if is_tombstone(rec.status()) && !show_tombs {
            continue;
        }
        if is_tombstone(rec.status()) && !show_tombs {
            continue;
        }
        if let Some(since) = &since {
            if rec.date() == "unknown" || rec.date() < since.as_str() {
                continue;
            }
        }
        let mut tokens = Vec::new();
        if let Some(name) = &want_part {
            // `--part not-verified`: search only that part, and only records that have it.
            let parts = record::parts_of(rec);
            let Some(found) = record::find_part(&parts, name) else {
                continue;
            };
            tokens.extend(search::terms(&found.text));
        } else {
            let title = search::terms(rec.title());
            for _ in 0..3 {
                tokens.extend(title.iter().cloned());
            }
            if let Some(short) = rec.fm.get_str("short") {
                tokens.extend(search::terms(short));
            }
            tokens.extend(search::terms(&rec.body));
        }
        docs.push((rec.id_str(), tokens));
        chosen.push(rec);
    }
    // A record that shares no word with the query is not a hit.
    let ranked: Vec<(String, f64)> = if listing {
        let mut all: Vec<&Record> = chosen.clone();
        // "unknown" sorts last, not above every date.
        let date = |r: &Record| if r.date() == "unknown" { "" } else { r.date() }.to_string();
        all.sort_by(|a, b| date(b).cmp(&date(a)).then(a.id_str().cmp(&b.id_str())));
        let n = all.len() as f64;
        all.iter()
            .enumerate()
            .map(|(i, r)| (r.id_str(), n - i as f64))
            .collect()
    } else {
        bm25(&docs, &query)
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .collect()
    };
    let mut hits: Vec<(String, f64, bool)> = ranked
        .into_iter()
        .map(|(id, score)| {
            let tomb = store
                .get(&id)
                .map(|r| is_tombstone(r.status()))
                .unwrap_or(false);
            (id, score, tomb)
        })
        .collect();
    hits.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.0.cmp(&b.0))
    });
    hits.truncate(limit);
    if hits.is_empty() {
        return Outcome {
            code: 1,
            stdout: String::new(),
            stderr: "no hits\n".into(),
        };
    }
    if cmd.format() == "json" {
        return ok(find_json(&store, &hits));
    }
    let mut stdout = String::new();
    for (id, _score, _) in &hits {
        let Some(rec) = store.get(id) else { continue };
        let label = rec
            .fm
            .get_str("short")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| rec.title());
        stdout.push_str(&format!(
            "{}  {}  {}  {}  {}\n",
            rec.id_str(),
            rec.type_name(),
            rec.date(),
            rec.areas().join(","),
            label
        ));
        if snippet {
            if let Some(line) = best_line(&rec.body, &query) {
                stdout.push_str(&format!("    {line}\n"));
            }
        }
    }
    ok(stdout)
}

fn context_cmd(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let budget = match cmd.opt("budget") {
        Some(s) => match s.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return usage("--budget needs a number"),
        },
        None => store.config.budgets.context_default,
    };
    let req = Request {
        task: cmd.positionals.join(" "),
        loop_id: cmd.opt("loop").map(|s| with_prefix("LOOP", &s)),
        review_id: cmd.opt("review").map(|s| with_prefix("REV", &s)),
        item_id: cmd.opt("item").map(|s| with_prefix("ITEM", &s)),
        core_only: cmd.bools.contains("core"),
        budget,
        areas: cmd.opt("area").map(|s| split_csv(&s)).unwrap_or_default(),
        explain: cmd.bools.contains("explain"),
        terms: cmd.opt("terms"),
    };
    match context::build(&store, &req) {
        Ok(bundle) => {
            if cmd.format() == "json" {
                ok(format!(
                    "{{\"markdown\":{},\"tok4\":{},\"budget\":{},\"included\":{},\"omitted\":{}}}\n",
                    util::json_escape(&bundle.markdown),
                    bundle.tok,
                    bundle.budget,
                    json_strings(&bundle.included),
                    json_strings(&bundle.omitted)
                ))
            } else {
                ok(bundle.markdown)
            }
        }
        Err(context::Fail::Usage(e)) => usage(e),
        Err(context::Fail::Missing(e)) => Outcome {
            code: 1,
            stdout: String::new(),
            stderr: format!("{e}\n"),
        },
        Err(context::Fail::Over(over)) => Outcome {
            code: 3,
            stdout: String::new(),
            stderr: format!(
                "{} {} tok4 > {}\n",
                over.rel,
                issue::thousands(over.tok),
                issue::thousands(over.budget)
            ),
        },
    }
}

/// `trailmark terms [<words>] [--all] [--budget N]`: the term index an agent rewrites its task
/// into before `trailmark context --terms`. Ids the store does not hold are left out and named on
/// stderr.
fn terms_cmd(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let budget = match cmd.opt("budget") {
        Some(s) => match s.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return usage("--budget needs a number"),
        },
        None => store.config.budgets.terms,
    };
    let terms = match crate::terms::load(&store) {
        Ok(t) => t,
        Err(e) => return usage(e),
    };
    if terms.is_empty() {
        return Outcome {
            code: 1,
            stdout: String::new(),
            stderr: format!(
                "no terms: {} is missing or empty\n",
                crate::terms::path(&store).display()
            ),
        };
    }
    let printed = crate::terms::render(
        &store,
        &terms,
        budget,
        cmd.bools.contains("all"),
        &cmd.positionals.join(" "),
    );
    let stderr = if printed.dead.is_empty() {
        String::new()
    } else {
        format!(
            "terms point to ids this store does not hold (left out): {}\n",
            printed.dead.join(", ")
        )
    };
    Outcome {
        code: 0,
        stdout: printed.text,
        stderr,
    }
}

fn add(root: &Path, cmd: &Cmd, stdin: &str) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let type_name = cmd.positionals.first().cloned().unwrap_or_default();
    if type_name.is_empty() {
        return usage("add needs a type");
    }
    let body = if cmd.opt("body").as_deref() == Some("-") {
        Some(stdin.to_string())
    } else if let Some(path) = cmd.opt("body-file").or(cmd.opt("body")) {
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) => return usage(format!("cannot read {path}: {e}")),
        }
    } else {
        None
    };
    let mut parts = Vec::new();
    for spec in cmd.values.get("part").cloned().unwrap_or_default() {
        let Some((name, path)) = spec.split_once('=') else {
            return usage("--part needs name=path");
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => return usage(format!("cannot read {path}: {e}")),
        };
        parts.push((name.to_string(), text));
    }
    let mut fields = Vec::new();
    for spec in cmd.values.get("field").cloned().unwrap_or_default() {
        let Some((key, value)) = spec.split_once('=') else {
            return usage("--field needs key=value");
        };
        fields.push((key.to_string(), value.to_string()));
    }
    for (flag, key) in [
        ("kind", "kind"),
        ("severity", "severity"),
        ("owner", "owner"),
        ("verdict", "verdict"),
    ] {
        if let Some(value) = cmd.opt(flag) {
            fields.push((key.into(), value));
        }
    }
    let req = AddRequest {
        type_name,
        id: cmd.opt("id"),
        key: cmd.positionals.get(1).cloned(),
        for_id: cmd.opt("for").map(|s| with_prefix("LOOP", &s)),
        title: cmd.opt("title"),
        body,
        areas: cmd.opt("areas").map(|s| split_csv(&s)).unwrap_or_default(),
        fields,
        parts,
        item: cmd.opt("item"),
        figures: cmd.values.get("figure").cloned().unwrap_or_default(),
        platform: cmd
            .opt("platform")
            .map(|s| split_csv(&s))
            .unwrap_or_default(),
    };
    match mutate::add(&store, &req) {
        Ok((next, written)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => {
                let mut stdout = format!("{}\n", written.id);
                for path in paths {
                    stdout.push_str(&path);
                    stdout.push('\n');
                }
                if cmd.format() != "json" {
                    if let Some(rec) = next.get(&written.id) {
                        for line in crate::relate::suggestions(&next, rec) {
                            stdout.push_str(&line);
                            stdout.push('\n');
                        }
                    }
                }
                if cmd.format() == "json" {
                    stdout = format!(
                        "{{\"id\":{},\"path\":{}}}\n",
                        util::json_escape(&written.id),
                        util::json_escape(&written.paths.first().cloned().unwrap_or_default())
                    );
                }
                ok(stdout)
            }
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn close(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(id) = cmd.positionals.first() else {
        return usage(
            "close <LOOP-KEY|KEY|REV-KEY> --gates \"…\" [--gates …] (--review REV-KEY|owed | --verdict go) [--part name=file]",
        );
    };
    let mut parts = Vec::new();
    for spec in cmd.values.get("part").cloned().unwrap_or_default() {
        let Some((name, path)) = spec.split_once('=') else {
            return usage("--part needs name=path");
        };
        match std::fs::read_to_string(path) {
            Ok(text) => parts.push((name.to_string(), text)),
            Err(e) => return usage(format!("cannot read {path}: {e}")),
        }
    }
    let mut fields = Vec::new();
    for spec in cmd.values.get("field").cloned().unwrap_or_default() {
        let Some((key, value)) = spec.split_once('=') else {
            return usage("--field needs key=value");
        };
        fields.push((key.to_string(), value.to_string()));
    }
    let req = mutate::CloseRequest {
        id: with_prefix("LOOP", id),
        gates: cmd.values.get("gates").cloned().unwrap_or_default(),
        review: cmd.opt("review").map(|r| {
            if r == "owed" {
                r
            } else {
                with_prefix("REV", &r)
            }
        }),
        verdict: cmd.opt("verdict"),
        parts,
        fields,
    };
    match mutate::close(&store, &req) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn link(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let from = cmd.positionals.first().cloned().unwrap_or_default();
    let to = cmd.positionals.get(1).cloned().unwrap_or_default();
    let rel = match cmd.opt("rel") {
        Some(r) => r,
        None => return usage("link needs --rel"),
    };
    if from.is_empty() || to.is_empty() {
        return usage("link needs <from-id> <to-id>");
    }
    match mutate::link(&store, &from, &to, &rel, cmd.bools.contains("remove")) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn retire(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = cmd.positionals.first().cloned().unwrap_or_default();
    if id.is_empty() {
        return usage("retire needs an id");
    }
    let why = match cmd.opt("why") {
        Some(w) => w,
        None => return usage("retire needs --why"),
    };
    let by = cmd.opt("by");
    match mutate::retire(&store, &id, &why, by.as_deref()) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn set(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    const USE: &str =
        "set <ITEM-or-OPEN-id> [status=<value>] --why \"<one line>\" [--by <id|author>]";
    let Some(id) = cmd.positionals.first() else {
        return usage(USE);
    };
    let mut pairs = Vec::new();
    for arg in &cmd.positionals[1..] {
        match arg.split_once('=') {
            Some((k, v)) if !k.is_empty() => pairs.push((k.to_string(), v.to_string())),
            _ => return usage(format!("{arg}: expected key=value; {USE}")),
        }
    }
    let why = cmd.opt("why").unwrap_or_default();
    // No key=value is a note on the status the record has.
    if pairs.is_empty() && why.trim().is_empty() {
        return usage(USE);
    }
    let by = cmd.opt("by");
    match mutate::set(&store, id, &pairs, &why, by.as_deref()) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn tag(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(id) = cmd.positionals.first() else {
        return usage("tag <id> --add <tag,tag> | --drop <tag>");
    };
    let list = |flag: &str| -> Vec<String> {
        cmd.values
            .get(flag)
            .cloned()
            .unwrap_or_default()
            .iter()
            .flat_map(|v| split_csv(v))
            .collect()
    };
    match mutate::tag(&store, id, &list("add"), &list("drop")) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn figure(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(id) = cmd.positionals.first() else {
        return usage("figure <id> --add \"<path> | <caption>\" | --drop <path>");
    };
    let add = cmd.values.get("add").cloned().unwrap_or_default();
    let remove = cmd.values.get("drop").cloned().unwrap_or_default();
    match mutate::figure(&store, id, &add, &remove) {
        Ok((next, _)) => match mutate::commit_store(root, &store, &next) {
            Ok(paths) => ok(paths.into_iter().map(|p| format!("{p}\n")).collect()),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn reserve(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let type_name = cmd.positionals.first().cloned().unwrap_or_default();
    let n: u32 = match cmd.positionals.get(1).and_then(|s| s.parse().ok()) {
        Some(n) => n,
        None => return usage("reserve needs <type> <n> --for <id>"),
    };
    let for_id = match cmd.opt("for") {
        Some(id) => with_prefix("LOOP", &id),
        None => return usage("reserve needs --for"),
    };
    match mutate::reserve(&store, &type_name, n, &for_id) {
        Ok((next, block)) => match mutate::commit_store(root, &store, &next) {
            Ok(_) => ok(format!(
                "[[reservation]]\ntype = \"{}\"\nfrom = {}\nto = {}\nfor = \"{}\"\ndate = \"{}\"\n",
                block.type_name, block.from, block.to, block.for_id, block.date
            )),
            Err(e) => mutate_fail(e),
        },
        Err(e) => mutate_fail(e),
    }
}

fn index(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let dir = store.config.store_dir(root);
    if cmd.bools.contains("check") {
        return match indexgen::plan(&store) {
            Ok(files) => {
                let mut stdout = String::new();
                let mut differ = false;
                for file in &files {
                    let path = dir.join(&file.rel);
                    let same = std::fs::read(&path).ok().as_deref() == Some(file.bytes.as_slice());
                    if !same {
                        differ = true;
                    }
                    stdout.push_str(&index_line(file));
                }
                Outcome {
                    code: if differ { 1 } else { 0 },
                    stdout,
                    stderr: String::new(),
                }
            }
            Err(overs) => over_budget(&overs),
        };
    }
    match indexgen::write_to(&store, &dir) {
        Ok(files) => {
            let mut stdout = String::new();
            for file in &files {
                stdout.push_str(&index_line(file));
            }
            ok(stdout)
        }
        Err(IndexFail::Over(overs)) => over_budget(&overs),
        Err(IndexFail::Io(e)) => Outcome {
            code: 4,
            stdout: String::new(),
            stderr: format!("{e}\n"),
        },
    }
}

fn check_cmd(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = CheckOpts {
        changed: cmd.bools.contains("changed"),
        history: cmd.bools.contains("history"),
        strict: cmd.bools.contains("strict"),
        indexes: true,
        code: true,
    };
    let result = check::check_store(&store, &opts);
    let (fail_n, _) = check::error_count(&result.problems, opts.strict);
    let stdout = if cmd.format() == "json" {
        check_json(&result, opts.strict)
    } else {
        check::render_text(&result)
    };
    Outcome {
        code: if fail_n == 0 { 0 } else { 1 },
        stdout,
        stderr: String::new(),
    }
}

fn why(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let target = cmd.positionals.first().cloned().unwrap_or_default();
    if target.is_empty() {
        return usage("why needs a path or a commit");
    }
    if is_commit(&target) {
        return why_commit(root, &store, &target);
    }
    if Id::parse(&target).is_some() && !store.root.join(&target).exists() {
        // An id is not a path: `trailmark why RULE-1` answered "cannot read RULE-1".
        return usage(format!(
            "why takes a path (`src/app.js:10-20`) or a commit and names the records cited there; for {target} itself use `trailmark show {target} --links`"
        ));
    }
    why_path(&store, &target)
}

fn why_path(store: &Store, target: &str) -> Outcome {
    let (path, start, end) = split_lines(target);
    let full = store.root.join(&path);
    let text = match std::fs::read_to_string(&full) {
        Ok(t) => t,
        Err(e) => {
            return Outcome {
                code: 1,
                stdout: String::new(),
                stderr: format!("cannot read {path}: {e}\n"),
            }
        }
    };
    let hits = crate::scan::scan_file(store, &full, &text, true, false);
    let mut stdout = String::new();
    let mut found = false;
    for hit in hits {
        if start > 0 && (hit.line < start || hit.line > end) {
            continue;
        }
        found = true;
        stdout.push_str(&format!(
            "{}:{}  cites {}\n",
            hit.path, hit.line, hit.written
        ));
        if hit.ids.is_empty() {
            let nearest = Id::parse(&hit.written)
                .map(|id| store.nearest(&id))
                .unwrap_or_default();
            let near = if nearest.is_empty() {
                String::new()
            } else {
                format!(" Nearest existing: {}.", nearest.join(", "))
            };
            stdout.push_str(&format!("    {} → no record (C10).{near}\n", hit.written));
        } else {
            for id in &hit.ids {
                stdout.push_str(&walk_source(store, id, 0));
            }
        }
    }
    if !found {
        Outcome {
            code: 1,
            stdout: String::new(),
            stderr: "nothing is cited\n".into(),
        }
    } else {
        ok(stdout)
    }
}

fn why_commit(root: &Path, store: &Store, hash: &str) -> Outcome {
    let message = crate::gitinfo::output(
        root,
        &["log".into(), "-1".into(), "--format=%B".into(), hash.into()],
    )
    .unwrap_or_default();
    if message.is_empty() {
        return Outcome {
            code: 1,
            stdout: String::new(),
            stderr: format!("cannot read commit {hash}\n"),
        };
    }
    let mut stdout = String::new();
    let mut found = false;
    if let Some(code) = subject_code().captures(&message).map(|c| c[1].to_string()) {
        let id = format!("LOOP-{code}");
        found = true;
        stdout.push_str(&format!("subject {code}: → {id}\n"));
        stdout.push_str(&walk_source(store, &id, 0));
    }
    let hits = crate::scan::scan_text(store, hash, &message, 1, true, false, None);
    for hit in hits {
        found = true;
        stdout.push_str(&format!("commit {hash}  cites {}\n", hit.written));
        if hit.ids.is_empty() {
            stdout.push_str(&format!("    {} → no record (C10).\n", hit.written));
        } else {
            for id in hit.ids {
                stdout.push_str(&walk_source(store, &id, 0));
            }
        }
    }
    if !found {
        Outcome {
            code: 1,
            stdout,
            stderr: "nothing is cited\n".into(),
        }
    } else {
        ok(stdout)
    }
}

fn walk_source(store: &Store, id: &str, depth: usize) -> String {
    if depth > 3 {
        return String::new();
    }
    let Some(rec) = store.get(id) else {
        return format!("    {id} → no record\n");
    };
    let mut out = format!("    {} → {}\n", rec.id_str(), rec.title());
    out.push_str(&crate::relate::why_lines(store, rec));
    if let Some(source) = rec.fm.get_str("source") {
        if !record::is_sentinel(source) && depth < 3 {
            out.push_str(&format!("    source {source}\n"));
            out.push_str(&walk_source(store, source, depth + 1));
        }
    }
    out
}

fn measure(root: &Path, cmd: &Cmd) -> Outcome {
    let store = match load(root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let planned = match indexgen::plan(&store) {
        Ok(files) => files,
        Err(overs) => return over_budget(&overs),
    };
    let mut stdout = String::new();
    let mut over = false;
    for file in &planned {
        stdout.push_str(&index_line(file));
        if let Some(budget) = file.budget {
            if file.tok > budget {
                over = true;
            }
        }
    }
    let tasks = match cmd.opt("tasks") {
        // `--tasks <file>`: a TOML file of `[[task]]` tables (name, text, areas).
        Some(path) => match read_tasks(&path) {
            Ok(t) => t,
            Err(e) => return usage(e),
        },
        None => store.config.measure_tasks.clone(),
    };
    for task in tasks {
        let req = Request {
            task: task.text,
            loop_id: None,
            review_id: None,
            item_id: None,
            core_only: false,
            budget: store.config.budgets.context_default,
            areas: task.areas,
            explain: false,
            terms: None,
        };
        match context::build(&store, &req) {
            Ok(bundle) => {
                stdout.push_str(&format!(
                    "context {}  {} / {} tok4\n",
                    task.name,
                    issue::thousands(bundle.tok),
                    issue::thousands(bundle.budget)
                ));
                if bundle.tok > bundle.budget {
                    over = true;
                }
                if let Some(bin) = cmd.opt("tokenizer") {
                    stdout.push_str(&tokenizer_note(&bin, &bundle.markdown));
                }
            }
            Err(context::Fail::Over(o)) => {
                over = true;
                stdout.push_str(&format!(
                    "context {}  {} tok4 > {}\n",
                    task.name,
                    issue::thousands(o.tok),
                    issue::thousands(o.budget)
                ));
            }
            Err(e) => {
                let msg = match e {
                    context::Fail::Usage(m) | context::Fail::Missing(m) => m,
                    context::Fail::Over(_) => String::new(),
                };
                stdout.push_str(&format!("context {}  {msg}\n", task.name));
            }
        }
    }
    Outcome {
        code: if over { 3 } else { 0 },
        stdout,
        stderr: String::new(),
    }
}

fn read_tasks(path: &str) -> Result<Vec<config::MeasureTask>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let arr = v
        .get("task")
        .and_then(|x| x.as_array())
        .ok_or_else(|| format!("{path}: no [[task]] tables"))?;
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        if let Some(t) = item.as_table() {
            for key in t.keys() {
                if !matches!(key.as_str(), "name" | "text" | "areas") {
                    return Err(format!("{path}: task {} has unknown key '{key}'", i + 1));
                }
            }
        }
        let text = item
            .get("text")
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("{path}: task {} has no text", i + 1))?;
        out.push(config::MeasureTask {
            name: item
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("task")
                .to_string(),
            text: text.to_string(),
            areas: item
                .get("areas")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

fn adapter_cmd(root: &Path, cmd: &Cmd) -> Outcome {
    if cmd.positionals.first().map(|s| s.as_str()) != Some("install") {
        return usage("adapter install <name>");
    }
    let name = cmd.positionals.get(1).cloned().unwrap_or_default();
    if name.is_empty() {
        return usage(format!(
            "adapter install <name>; this build carries: {}",
            crate::adapter::names().join(", ")
        ));
    }
    let result = crate::adapter::install(
        root,
        &name,
        cmd.bools.contains("dry-run"),
        cmd.bools.contains("force"),
    );
    Outcome {
        code: result.code,
        stdout: result.stdout,
        stderr: result.stderr,
    }
}

fn load(root: &Path) -> Result<Store, Outcome> {
    Store::load(root).map_err(|e| Outcome {
        code: 4,
        stdout: String::new(),
        stderr: format!("{e}\n"),
    })
}

fn missing(store: &Store, token: &str) -> String {
    let near = Id::parse(token)
        .map(|id| store.nearest(&id))
        .unwrap_or_default();
    if near.is_empty() {
        format!("unknown id {token}\n")
    } else {
        format!("unknown id {token}\ndid you mean {}?\n", near.join(", "))
    }
}

fn canon_or(id: &str) -> String {
    Id::parse(id)
        .map(|i| i.canonical())
        .unwrap_or_else(|| id.to_string())
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::List(xs) => xs.iter().map(value_text).collect::<Vec<_>>().join(", "),
    }
}

fn best_line(body: &str, query: &[String]) -> Option<String> {
    body.lines()
        .map(|line| {
            let tokens = search::terms(line);
            let n = query
                .iter()
                .filter(|q| tokens.iter().any(|t| t == *q))
                .count();
            (n, line.to_string())
        })
        .filter(|(n, _)| *n > 0)
        .max_by_key(|(n, line)| (*n, std::cmp::Reverse(line.len())))
        .map(|(_, line)| line)
}

fn cut_at(text: &str, budget: usize, hint: &str) -> String {
    if tok4(text) <= budget {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if tok4(&out) >= budget {
            break;
        }
    }
    format!(
        "{out}\n[cut at {} tok4 of {} — {hint}]\n",
        issue::thousands(budget),
        issue::thousands(tok4(text))
    )
}

fn index_line(file: &Planned) -> String {
    let mut line = match file.budget {
        Some(budget) => format!(
            "{}  {} records  {} / {} tok4",
            file.rel,
            file.records,
            issue::thousands(file.tok),
            issue::thousands(budget)
        ),
        None => format!(
            "{}  {} records  {} tok4",
            file.rel,
            file.records,
            issue::thousands(file.tok)
        ),
    };
    if let Some(note) = &file.folded {
        line.push_str(&format!("  folded: {note}"));
    }
    line.push('\n');
    line
}

fn over_budget(overs: &[indexgen::Over]) -> Outcome {
    let mut stderr = String::new();
    for over in overs {
        stderr.push_str(&format!(
            "{} {} tok4 > {}\n",
            over.rel,
            issue::thousands(over.tok),
            issue::thousands(over.budget)
        ));
    }
    Outcome {
        code: 3,
        stdout: String::new(),
        stderr,
    }
}

fn mutate_fail(err: mutate::Fail) -> Outcome {
    let (code, message) = match err {
        mutate::Fail::Usage(m) => (2, m),
        mutate::Fail::Missing(m) => (1, m),
        mutate::Fail::Conflict(m) => (5, m),
        mutate::Fail::Invalid(m) => (6, m),
        mutate::Fail::Frozen(m) => (7, m),
    };
    Outcome {
        code,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

fn split_lines(target: &str) -> (String, usize, usize) {
    if let Some((path, rest)) = target.rsplit_once(':') {
        if let Some((a, b)) = rest.split_once('-') {
            if let (Ok(start), Ok(end)) = (a.parse::<usize>(), b.parse::<usize>()) {
                return (path.to_string(), start, end);
            }
        }
        if let Ok(line) = rest.parse::<usize>() {
            return (path.to_string(), line, line);
        }
    }
    (target.to_string(), 0, usize::MAX)
}

fn is_commit(token: &str) -> bool {
    token.len() >= 7
        && token.len() <= 40
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && !token.contains('/')
        && !token.contains('.')
}

fn subject_code() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^([A-Z]{1,4}\d+[a-z]?):").unwrap())
}

fn tokenizer_note(bin: &str, text: &str) -> String {
    let mut parts = bin.split_whitespace();
    let Some(prog) = parts.next() else {
        return String::new();
    };
    let args: Vec<&str> = parts.collect();
    let mut child = match std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("tokenizer: {e}\n"),
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) => format!(
            "tokenizer {}\n",
            String::from_utf8_lossy(&out.stdout).trim()
        ),
        Err(e) => format!("tokenizer: {e}\n"),
    }
}

fn show_json(store: &Store, tokens: &[String]) -> String {
    let mut records = String::from("[");
    let mut first = true;
    for token in tokens {
        let ids = match store.resolve_token(token, true) {
            Resolved::One(id) => vec![id],
            Resolved::Range(ids) => ids,
            _ => continue,
        };
        for id in ids {
            let Some(rec) = store.get(&id.canonical()) else {
                continue;
            };
            if !first {
                records.push(',');
            }
            first = false;
            let parts = record::parts_of(rec);
            let mut part_json = String::from("[");
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    part_json.push(',');
                }
                part_json.push_str(&format!(
                    "{{\"name\":{},\"text\":{}}}",
                    util::json_escape(&part.name),
                    util::json_escape(&part.text)
                ));
            }
            part_json.push(']');
            records.push_str(&format!(
                "{{\"id\":{},\"type\":{},\"title\":{},\"frontmatter\":{},\"body\":{},\"parts\":{}}}",
                util::json_escape(&rec.id_str()),
                util::json_escape(rec.type_name()),
                util::json_escape(rec.title()),
                fm_json(rec),
                util::json_escape(&rec.body),
                part_json
            ));
        }
    }
    records.push(']');
    format!("{{\"records\":{records}}}\n")
}

fn fm_json(rec: &Record) -> String {
    let mut out = String::from("{");
    for (i, (key, value)) in rec.fm.keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&util::json_escape(key));
        out.push(':');
        out.push_str(&value_json(value));
    }
    out.push('}');
    out
}

fn value_json(value: &Value) -> String {
    match value {
        Value::String(s) => util::json_escape(s),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::List(xs) => {
            let inner: Vec<String> = xs.iter().map(value_json).collect();
            format!("[{}]", inner.join(","))
        }
    }
}

fn find_json(store: &Store, hits: &[(String, f64, bool)]) -> String {
    let mut body = String::from("{\"hits\":[");
    for (i, (id, score, _)) in hits.iter().enumerate() {
        let Some(rec) = store.get(id) else { continue };
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            "{{\"id\":{},\"type\":{},\"date\":{},\"areas\":{},\"title\":{},\"score\":{score}}}",
            util::json_escape(&rec.id_str()),
            util::json_escape(rec.type_name()),
            util::json_escape(rec.date()),
            json_strings(&rec.areas()),
            util::json_escape(rec.title())
        ));
    }
    body.push_str("]}\n");
    body
}

fn check_json(result: &check::CheckResult, strict: bool) -> String {
    let errors = result
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    let warnings = result
        .problems
        .iter()
        .filter(|p| p.severity == Severity::Warning)
        .count();
    let mut problems = String::from("[");
    for (i, p) in result.problems.iter().enumerate() {
        if i > 0 {
            problems.push(',');
        }
        problems.push_str(&format!(
            "{{\"path\":{},\"line\":{},\"code\":{},\"severity\":{},\"message\":{}}}",
            util::json_escape(&p.path),
            p.line,
            util::json_escape(p.code),
            util::json_escape(p.severity.as_str()),
            util::json_escape(&p.message)
        ));
    }
    problems.push(']');
    let _ = strict;
    format!("{{\"errors\":{errors},\"warnings\":{warnings},\"problems\":{problems}}}\n")
}

fn json_strings(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| util::json_escape(s)).collect();
    format!("[{}]", inner.join(","))
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

impl Cmd {
    fn opt(&self, name: &str) -> Option<String> {
        self.values.get(name).and_then(|v| v.last().cloned())
    }
    fn format(&self) -> String {
        self.opt("format").unwrap_or_else(|| "md".into())
    }
}

fn ok(stdout: String) -> Outcome {
    Outcome {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn usage(message: impl Into<String>) -> Outcome {
    let mut stderr = message.into();
    if !stderr.ends_with('\n') {
        stderr.push('\n');
    }
    Outcome {
        code: 2,
        stdout: String::new(),
        stderr,
    }
}
