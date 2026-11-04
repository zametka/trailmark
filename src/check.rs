//! Checker rules C01–C19.
//!
//! Two deviations keep `trailmark check` able to pass while records exist and the
//! old documents are still in the tree: C18 runs only when `trail.toml` has
//! `cutover = true`, and C06 ignores fenced code and any heading that is
//! already the first line of a rule body.

use crate::gitinfo;
use crate::id::{self, Id};
use crate::indexgen;
use crate::issue::{thousands, Problem, Severity};
use crate::record::{self, is_sentinel, is_tombstone, Record, KNOWN_KEYS};
use crate::scan::{self, Hit};
use crate::store::{self, Store};
use crate::util::{self, tok4};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::OnceLock;

pub struct CheckOpts {
    pub changed: bool,
    pub history: bool,
    pub strict: bool,
    pub indexes: bool,
    pub code: bool,
}

impl Default for CheckOpts {
    fn default() -> Self {
        Self {
            changed: false,
            history: false,
            strict: false,
            indexes: true,
            code: true,
        }
    }
}

pub struct CheckResult {
    pub problems: Vec<Problem>,
    pub records: usize,
    pub code_refs: usize,
    /// Every waiver in force, one line each, so a waived finding is never invisible.
    pub waivers: Vec<String>,
}

pub fn check_store(store: &Store, opts: &CheckOpts) -> CheckResult {
    let mut problems = store.broken.clone();
    let focus = if opts.changed {
        changed_set(store)
    } else {
        None
    };
    let want = |rel: &str| focus.as_ref().map(|s| s.contains(rel)).unwrap_or(true);

    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rec in &store.records {
        if let Some(id) = rec.id() {
            ids.entry(id.canonical()).or_default().push(rec.rel.clone());
        }
        if let Some(id) = rec.id() {
            for alias in rec.fm.strings("aliases") {
                aliases
                    .entry(store::compact(&alias))
                    .or_default()
                    .push(id.canonical());
            }
        }
    }
    for (id, paths) in &ids {
        if paths.len() > 1 {
            problems.push(Problem::error(
                "C04",
                "",
                0,
                format!("{id} defined twice: {}", paths.join(", ")),
            ));
        }
    }
    for (alias, owners) in &aliases {
        let mut uniq = owners.clone();
        uniq.sort();
        uniq.dedup();
        if uniq.len() > 1 {
            problems.push(Problem::error(
                "C04",
                "",
                0,
                format!("alias '{alias}' is shared by {}", uniq.join(", ")),
            ));
        }
        if let Some(id) = Id::parse(alias) {
            if ids.contains_key(&id.canonical()) && !uniq.iter().any(|o| o == &id.canonical()) {
                problems.push(Problem::error(
                    "C04",
                    "",
                    0,
                    format!("alias '{alias}' equals id {}", id.canonical()),
                ));
            }
        }
    }

    let mut pins = 0usize;
    for rec in &store.records {
        if rec.pinned() {
            pins += 1;
        }
        if !want(&rec.rel) {
            continue;
        }
        problems.extend(check_record(store, rec));
    }
    if pins > 40 {
        problems.push(Problem::error(
            "C16",
            "",
            0,
            format!("{pins} pinned records; the maximum is 40"),
        ));
    }

    problems.extend(check_reservations(store, &want));
    problems.extend(check_relations(store));
    problems.extend(check_live_deploys(store));
    problems.extend(check_outside_rules(store));

    let code_hits = if opts.code {
        code_hits(store)
    } else {
        Vec::new()
    };
    let code_refs = code_hits.len();
    if opts.code {
        problems.extend(check_code(store, &code_hits));
        problems.extend(check_tombstone_cites(store, &code_hits, &want));
        problems.extend(check_code_scanned(store, code_refs));
    } else {
        problems.extend(check_tombstone_cites(store, &[], &want));
    }

    if opts.indexes {
        match indexgen::plan(store) {
            Ok(files) if store.config.commit_index => problems.extend(check_fresh(store, &files)),
            Ok(_) => problems.extend(check_index_untracked(store)),
            Err(overs) => {
                for over in overs {
                    problems.push(Problem::error(
                        "C09",
                        "",
                        0,
                        format!(
                            "{} {} tok4 > {}",
                            over.rel,
                            thousands(over.tok),
                            thousands(over.budget)
                        ),
                    ));
                }
            }
        }
    }

    if opts.history {
        problems.extend(check_history(store));
    }
    problems.extend(check_hashes(store, &want));
    if store.config.cutover {
        problems.extend(check_stubs(store));
    }

    problems.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.code.cmp(b.code))
            .then(a.message.cmp(&b.message))
    });
    problems.dedup_by(|a, b| {
        a.path == b.path && a.line == b.line && a.code == b.code && a.message == b.message
    });
    let waivers = store
        .waivers
        .iter()
        .map(|w| {
            let at = match (w.path.is_empty(), w.line) {
                (true, _) => String::from("anywhere"),
                (false, 0) => w.path.clone(),
                (false, n) => format!("{}:{n}", w.path),
            };
            let what = if w.written.is_empty() {
                String::new()
            } else {
                format!(" {}", w.written)
            };
            format!(
                "waived {} at {at}{what} — {} ({}, {})",
                w.code, w.why, w.by, w.date
            )
        })
        .collect();
    CheckResult {
        waivers,
        problems,
        records: store.records.len(),
        code_refs,
    }
}

pub fn error_count(problems: &[Problem], strict: bool) -> (usize, usize) {
    let errors = problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    let warnings = problems
        .iter()
        .filter(|p| p.severity == Severity::Warning)
        .count();
    let fail = errors + if strict { warnings } else { 0 };
    (fail, warnings)
}

pub fn render_text(result: &CheckResult) -> String {
    let mut out = String::new();
    for p in &result.problems {
        out.push_str(&p.render());
        out.push('\n');
    }
    for w in &result.waivers {
        out.push_str(w);
        out.push('\n');
    }
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
    out.push_str(&format!(
        "{} {}, {} {}, {} records, {} code references\n",
        thousands(errors),
        plural(errors, "error", "errors"),
        thousands(warnings),
        plural(warnings, "warning", "warnings"),
        thousands(result.records),
        thousands(result.code_refs),
    ));
    out
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

fn check_record(store: &Store, rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    out.extend(check_keys(rec));
    out.extend(check_shape(store, rec));
    out.extend(check_links(store, rec));
    out.extend(check_parts(rec));
    out.extend(check_workflow(store, rec));
    out.extend(check_figures(store, rec));
    if let Some(limit) = store.config.budgets.record_warn.get(rec.type_name()) {
        let text = String::from_utf8_lossy(&rec.raw);
        let n = tok4(&text);
        if n > *limit {
            out.push(Problem::warning(
                "C14",
                rec.rel.clone(),
                1,
                format!(
                    "{} tok4 > {} — split the incident into INC-?",
                    thousands(n),
                    thousands(*limit)
                ),
            ));
        }
    }
    // A recipe is printed whole into bundles ("How to run"): a page of prose there costs
    // every task a rule.
    if rec.type_name() == "fact" && rec.fm.get_str("kind") == Some("recipe") {
        let n = tok4(&rec.body);
        if n > RECIPE_WARN {
            out.push(Problem::warning(
                "C14",
                rec.rel.clone(),
                1,
                format!(
                    "a recipe of {} tok4 > {RECIPE_WARN} — keep the commands, move the reasons to a rule",
                    thousands(n)
                ),
            ));
        }
    }
    out
}

/// The size past which a recipe's body draws a C14 warning.
const RECIPE_WARN: usize = 400;

fn check_keys(rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    for (key, _) in &rec.fm.keys {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            let hint = util::suggest(key, KNOWN_KEYS.iter().copied());
            let message = match hint {
                Some(s) => format!("unknown key '{key}' (did you mean '{s}'?)"),
                None => format!("unknown key '{key}'"),
            };
            out.push(Problem::error(
                "C01",
                rec.rel.clone(),
                rec.field_line(key),
                message,
            ));
        }
    }
    out
}

fn check_shape(store: &Store, rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    let id = rec.id();
    let Some(id) = id else {
        out.push(Problem::error(
            "C03",
            rec.rel.clone(),
            rec.field_line("id"),
            "id is missing or malformed",
        ));
        return out;
    };
    if !id::type_prefix_ok(rec.type_name(), &id.prefix) || !id::grammar_ok(&id, rec.type_name()) {
        out.push(Problem::error(
            "C03",
            rec.rel.clone(),
            rec.field_line("id"),
            format!(
                "id {} does not match the {} grammar",
                id.canonical(),
                rec.type_name()
            ),
        ));
    }
    let local = rec.rel.starts_with("local/");
    if let Some(expected) = id::rel_path(&id, rec.type_name(), local) {
        if expected != rec.rel {
            let file = rec.rel.rsplit('/').next().unwrap_or(&rec.rel);
            out.push(Problem::error(
                "C03",
                rec.rel.clone(),
                1,
                format!("id {} does not match file {file}", id.canonical()),
            ));
        }
    }
    if rec.type_name() == "fact" {
        let scope = rec.fm.get_str("scope").unwrap_or("");
        if scope == "local" && !rec.rel.starts_with("local/") {
            out.push(Problem::error(
                "C03",
                rec.rel.clone(),
                rec.field_line("scope"),
                "scope local must live under local/",
            ));
        }
        if rec.rel.starts_with("local/") && scope != "local" && !scope.is_empty() {
            out.push(Problem::error(
                "C03",
                rec.rel.clone(),
                rec.field_line("scope"),
                "a record under local/ needs scope local",
            ));
        }
    }

    let legacy = rec.legacy();
    require_str(&mut out, rec, "id");
    require_str(&mut out, rec, "type");
    require_str(&mut out, rec, "title");
    require_str(&mut out, rec, "date");
    require_str(&mut out, rec, "status");

    if let Some(title) = rec.fm.get_str("title") {
        if title.chars().count() > 200 {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("title"),
                "title is longer than 200 characters",
            ));
        }
    }
    if let Some(short) = rec.fm.get_str("short") {
        if short.chars().count() > 72 {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("short"),
                "short is longer than 72 characters",
            ));
        }
    }
    if let Some(date) = rec.fm.get_str("date") {
        if !util::valid_date(date) {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("date"),
                format!("date '{date}' is not YYYY-MM-DD"),
            ));
        } else if date == "unknown" && !legacy && rec.fm.get_str("date_note").is_none() {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("date"),
                "date unknown needs date_note",
            ));
        }
    }
    if let Some(status) = rec.fm.get_str("status") {
        let allowed = statuses(rec.type_name());
        if !allowed.is_empty() && !allowed.contains(&status) {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("status"),
                format!("status '{status}' not one of {}", allowed.join(", ")),
            ));
        }
    }
    if let Some(time) = rec.fm.get_str("time") {
        if !util::valid_time(time) {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("time"),
                format!("time '{time}' is not YYYY-MM-DDTHH:MMZ"),
            ));
        }
    }
    if !store.config.areas.is_empty() {
        for area in rec.areas() {
            if !store.config.areas.contains_key(&area) {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    rec.field_line("areas"),
                    // The refusal names the areas there are, or the next try is a guess too.
                    format!(
                        "area '{area}' is not in trail.toml; the areas are: {}",
                        store
                            .config
                            .areas
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    enum_field(&mut out, rec, "basis", &["word", "silence", "ruling"]);
    if let Some(scope) = rec.fm.get_str("scope") {
        let ok = match rec.type_name() {
            "fact" => matches!(scope, "repo" | "local"),
            _ => {
                matches!(scope, "product" | "process" | "repo" | "local")
                    || store.config.areas.contains_key(scope)
                    || store.config.areas.is_empty()
            }
        };
        if !ok {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("scope"),
                format!("scope '{scope}' is not valid for this type"),
            ));
        }
    }
    enum_field(&mut out, rec, "severity", &["high", "medium", "low"]);
    enum_field(&mut out, rec, "target", &["web", "mobile", "infra"]);
    enum_field(
        &mut out,
        rec,
        "verdict",
        &["go", "go-with-fixes", "hold", "no-go"],
    );
    if rec.type_name() == "open" {
        enum_field(
            &mut out,
            rec,
            "kind",
            &["risk", "follow-up", "owed-step", "unverified", "bug"],
        );
    }
    if rec.type_name() == "fact" {
        enum_field(
            &mut out,
            rec,
            "kind",
            // `recipe`: commands to copy, shown under "How to run" in a bundle.
            &[
                "env", "trap", "device", "tool", "process", "state", "recipe",
            ],
        );
    }
    // `applies_to`: areas, platforms, or situation words; paths in a migrated fact.
    for value in rec.fm.strings("applies_to") {
        if !crate::mutate::valid_tag(&value) && !value.contains('/') {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("applies_to"),
                format!("applies_to '{value}': lowercase words joined by hyphens, or a path"),
            ));
        }
    }
    for value in rec.fm.strings("platform") {
        if !["web", "server", "mobile", "infra", "docs"].contains(&value.as_str()) {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("platform"),
                format!("platform '{value}' not one of web, server, mobile, infra, docs"),
            ));
        }
    }
    if let Some(quote) = rec.fm.get_str("quote") {
        if quote.chars().count() > 300 {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("quote"),
                "quote is longer than 300 characters",
            ));
        }
    }
    if let Some(word) = rec.fm.get_str("word") {
        if word.chars().count() > 300 {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                rec.field_line("word"),
                "word is longer than 300 characters",
            ));
        }
    }

    if legacy {
        return out;
    }
    match rec.type_name() {
        "rule" => {
            if rec.areas().is_empty() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "areas is required",
                ));
            }
            if rec.fm.get_str("source").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "source is required",
                ));
            }
        }
        "decision" => {
            if rec.fm.get_str("decided_by").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "decided_by is required",
                ));
            }
            match rec.fm.get_str("basis") {
                None => out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "basis is required",
                )),
                Some("word") if rec.fm.get_str("quote").is_none() => {
                    out.push(Problem::error(
                        "C02",
                        rec.rel.clone(),
                        1,
                        "quote is required when basis is word",
                    ));
                }
                _ => {}
            }
        }
        "discussion" if rec.fm.get_str("from").is_none() => {
            out.push(Problem::error(
                "C02",
                rec.rel.clone(),
                1,
                "from is required",
            ));
        }
        "item" => {
            if rec.fm.get_str("asked_by").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "asked_by is required",
                ));
            }
            if rec.status() == "waiting" && rec.fm.get_str("waiting_on").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "waiting_on is required when status is waiting",
                ));
            }
            if rec.status() == "queued" && rec.fm.get("order").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "order is required when status is queued",
                ));
            }
        }
        "loop" => {
            if rec.fm.get_str("item").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "item is required",
                ));
            }
            if rec.fm.get("platform").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "platform is required",
                ));
            }
            if rec.fm.get_str("started").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "started is required",
                ));
            }
        }
        "review" => {
            if rec.fm.strings("reviews").is_empty() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "reviews is required",
                ));
            }
            // The review record is written when the review starts, before there is a
            // verdict. Verdict and gates are owed when it is done.
            if rec.status() == "done" && rec.fm.get_str("verdict").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "verdict is required when status is done",
                ));
            }
            if rec.status() == "done" && rec.fm.get("gates").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "gates is required when status is done",
                ));
            }
        }
        "open" => {
            for key in ["kind", "severity", "owner", "opened_by"] {
                if rec.fm.get_str(key).is_none() {
                    out.push(Problem::error(
                        "C02",
                        rec.rel.clone(),
                        1,
                        format!("{key} is required"),
                    ));
                }
            }
            if rec.areas().is_empty() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "areas is required",
                ));
            }
        }
        "deploy" => {
            for key in [
                "target",
                "time",
                "authorized_by",
                "word",
                "word_date",
                "backup",
            ] {
                if rec.fm.get_str(key).unwrap_or("").is_empty() {
                    out.push(Problem::error(
                        "C02",
                        rec.rel.clone(),
                        1,
                        format!("{key} is required"),
                    ));
                }
            }
        }
        "fact" => {
            if rec.fm.get_str("kind").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "kind is required",
                ));
            }
            if rec.fm.get_str("scope").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "scope is required",
                ));
            }
        }
        "incident" => {
            if rec.fm.get_str("severity").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "severity is required",
                ));
            }
            if rec.fm.get("production").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "production is required",
                ));
            }
            if rec.fm.get_str("found_by").is_none() {
                out.push(Problem::error(
                    "C02",
                    rec.rel.clone(),
                    1,
                    "found_by is required",
                ));
            }
        }
        _ => {}
    }
    out
}

/// C23: a scan that found nothing reads like a clean scan. A dead id in a folder `[code] include`
/// does not name is unreported, and the summary only says `0 code references`. The finding
/// belongs to `trail.toml`, which is where the fix goes.
fn check_code_scanned(store: &Store, code_refs: usize) -> Option<Problem> {
    if code_refs > 0 || store.config.code_scan_off {
        return None;
    }
    const FIX: &str = "set [code] include in trail.toml to the folders that hold your code, or include = [] if this repo has none";
    let include = store.config.code_include.join(", ");
    let found = if include.is_empty() {
        "trail.toml has no [code] include, so ids cited in code are not checked".to_string()
    } else {
        match scan::code_files(store).len() {
            0 => format!(
                "no file matched [code] include ({include}), so ids cited in code are not checked"
            ),
            1 => format!("1 file matched [code] include ({include}) and it cites no id"),
            n => format!("{n} files matched [code] include ({include}) and none cites an id"),
        }
    };
    Some(Problem::warning(
        "C23",
        "trail.toml",
        0,
        format!("{found}; {FIX}"),
    ))
}

/// C20: a figure is a picture or a short film under the store, with a caption that says what
/// to see. The file must exist, so a convention never points at a picture nobody can open.
fn check_figures(store: &Store, rec: &Record) -> Vec<Problem> {
    const KINDS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "mp4", "webm"];
    const FILM_MAX: u64 = 20 * 1024 * 1024;
    let mut out = Vec::new();
    let line = rec.field_line("figures");
    for raw in rec.fm.strings("figures") {
        let (path, caption) = record::split_figure(&raw);
        let mut bad = |m: String| out.push(Problem::error("C20", rec.rel.clone(), line, m));
        if caption.is_empty() {
            bad(format!(
                "figure {path} has no caption (write '<path> | <what to see>')"
            ));
        }
        if path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|p| p == ".." || p.is_empty())
        {
            bad(format!("figure '{path}' must be a path inside the store"));
            continue;
        }
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if !KINDS.contains(&ext.as_str()) {
            bad(format!("figure {path} is not one of {}", KINDS.join(", ")));
        }
        let full = store.config.store_dir(&store.root).join(&path);
        match std::fs::metadata(&full) {
            Err(_) => bad(format!(
                "figure {path} does not exist in the store: a figure path is relative to the store, so this looked for {}/{path}",
                store.config.store_rel
            )),
            Ok(m) if matches!(ext.as_str(), "mp4" | "webm") && m.len() > FILM_MAX => {
                bad(format!("figure {path} is over 20 MB; keep films short"))
            }
            Ok(_) => {}
        }
    }
    out
}

fn require_str(out: &mut Vec<Problem>, rec: &Record, key: &str) {
    if rec.fm.get_str(key).unwrap_or("").is_empty() {
        out.push(Problem::error(
            "C02",
            rec.rel.clone(),
            rec.field_line(key),
            format!("{key} is required"),
        ));
    }
}

fn enum_field(out: &mut Vec<Problem>, rec: &Record, key: &str, allowed: &[&str]) {
    let Some(value) = rec.fm.get_str(key) else {
        return;
    };
    if value.is_empty() {
        return;
    }
    if !allowed.contains(&value) {
        out.push(Problem::error(
            "C02",
            rec.rel.clone(),
            rec.field_line(key),
            format!("{key} '{value}' not one of {}", allowed.join(", ")),
        ));
    }
}

pub(crate) fn statuses(type_name: &str) -> Vec<&str> {
    let mut base: Vec<&str> = match type_name {
        "rule" | "fact" | "charter" => vec!["active"],
        "decision" => vec!["active", "revoked"],
        "discussion" => vec!["open", "triaged", "done"],
        "item" => vec![
            "queued", "running", "built", "waiting", "parked", "done", "dropped",
        ],
        "loop" => vec!["running", "done", "abandoned"],
        "review" => vec!["running", "done"],
        "open" => vec!["open", "closed", "wontfix"],
        "deploy" => vec!["live", "rolled-back"],
        "incident" => vec!["open", "resolved"],
        _ => vec![],
    };
    base.push("retired");
    base.push("superseded");
    base
}

fn check_links(store: &Store, rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut hits = scan::field_hits(store, rec);
    hits.extend(scan::body_hits(store, rec));
    let same = same_lines(&hits);
    for hit in &hits {
        if store.waived_hit("C05", &hit.path, hit.line, &hit.written, &same(hit)) {
            continue;
        }
        if hit.ambiguous {
            out.push(Problem::error(
                "C05",
                rec.rel.clone(),
                hit.line,
                format!("{} is ambiguous: {}", hit.written, hit.ids.join(", ")),
            ));
            continue;
        }
        if hit.informal
            && rec.legacy()
            && store.config.legacy_context_refs == "skip"
            && hit.ids.iter().all(|id| store.get(id).is_none())
        {
            // A deliberate policy (trail.toml `[check] legacy_context_refs = "skip"`):
            // migrated prose uses `#N` for deploys, reviews and builds alike, and the text
            // cannot be edited. New records are still checked.
            continue;
        }
        if hit.ids.is_empty() {
            let message = if hit.written.contains("..")
                || hit.written.contains('–')
                || hit.written.contains('-') && hit.written.contains('§')
            {
                format!("range resolves to nothing ({})", hit.written)
            } else {
                format!("dead link {}", hit.written)
            };
            out.push(Problem::error(
                "C05",
                rec.rel.clone(),
                hit.line,
                with_field(message, hit),
            ));
            continue;
        }
        for id in &hit.ids {
            if store.get(id).is_none() {
                let mut message = format!("dead link {id}");
                if hit.written != *id {
                    message = format!("dead link {} ({id})", hit.written);
                }
                out.push(Problem::error(
                    "C05",
                    rec.rel.clone(),
                    hit.line,
                    with_field(message, hit),
                ));
            } else if let Some(part) = &hit.part {
                if let Some(target) = store.get(id) {
                    if !scan::part_exists(target, part) {
                        out.push(Problem::warning(
                            "C05",
                            rec.rel.clone(),
                            hit.line,
                            format!("{id}#{part} has no such part"),
                        ));
                    }
                }
            }
        }
    }
    for (_, line, token) in scan::path_targets(rec) {
        // An absolute path in old prose is a path on some machine or device, not a link.
        if token.starts_with('/') {
            continue;
        }
        if let Err(message) =
            scan::path_exists(&store.root, &token, rec.fm.get_str("migrated_from"))
        {
            if rec.legacy() {
                // The text is history: the file it named may have moved since. Reported,
                // not failed; a new record's link must exist.
                out.push(Problem::warning("C05", rec.rel.clone(), line, message));
            } else {
                out.push(Problem::error("C05", rec.rel.clone(), line, message));
            }
        }
    }
    out
}

fn with_field(message: String, hit: &Hit) -> String {
    match &hit.field {
        Some(field) => format!("{message} (field {field})"),
        None => message,
    }
}

fn check_parts(rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    if rec.legacy() {
        return out;
    }
    // `trailmark add` writes a part it was not given as `Pending.` (the template's placeholder).
    // A running loop or review fills its parts at `trailmark close`; any other record is not
    // edited after it is added, so a placeholder there is a rule, an open item or a decision
    // with no text, live in every bundle, and nothing said so. A warning: `add`
    // still writes it, and `check` names it.
    let fills_later = matches!(rec.type_name(), "loop" | "review") && rec.status() == "running";
    if !fills_later {
        for (i, line) in rec.body.lines().enumerate() {
            let t = line.trim();
            if t == "Pending." || t.ends_with("** Pending.") {
                out.push(Problem::warning(
                    "C12",
                    rec.rel.clone(),
                    rec.body_line + i,
                    "a part still says Pending. — a record that is not a running loop or review was added without it (--part <name>=<file>)",
                ));
            }
        }
    }
    let parts = record::parts_of(rec);
    let has = |name: &str| record::find_part(&parts, name).is_some();
    let need = |out: &mut Vec<Problem>, name: &str, message: &str| {
        if !has(name) {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                0,
                message.to_string(),
            ));
        }
    };
    match rec.type_name() {
        "rule" => {
            if !has("incident") {
                out.push(Problem::error(
                    "C07",
                    rec.rel.clone(),
                    0,
                    "no incident part",
                ));
            }
            if !has("rule") {
                out.push(Problem::error(
                    "C07",
                    rec.rel.clone(),
                    0,
                    "no '> **Rule.**' block",
                ));
            }
        }
        "loop" => {
            for (name, label) in [
                ("brief", "Brief"),
                ("built", "Built"),
                ("verified", "Verified"),
                ("gates", "Gates"),
                ("not-verified", "Not verified"),
                ("lessons", "Lessons"),
            ] {
                need(&mut out, name, &format!("missing ## {label}"));
            }
        }
        "review" => {
            need(&mut out, "verdict", "missing ## Verdict");
            need(&mut out, "findings", "missing ## Findings");
            need(
                &mut out,
                "read-and-run-no-change",
                "missing ## Read and run, no change",
            );
            need(&mut out, "gates", "missing ## Gates");
            need(&mut out, "not-verified", "missing ## Not verified");
        }
        "open" => {
            need(&mut out, "what", "missing ## What");
            need(&mut out, "how-to-close", "missing ## How to close");
            if rec.status() == "wontfix" {
                need(&mut out, "why-not", "missing ## Why not");
            }
        }
        "deploy" => {
            need(&mut out, "verified", "missing ## Verified");
            need(&mut out, "not-verified", "missing ## Not verified");
        }
        "decision" => {
            need(&mut out, "decision", "missing ## Decision");
            need(&mut out, "why", "missing ## Why");
        }
        "fact" if (!rec.body.contains("**Why:**") || !rec.body.contains("**How to apply:**")) => {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                0,
                "a fact needs **Why:** and **How to apply:**",
            ));
        }
        "incident" => {
            for (name, label) in [
                ("what-happened", "What happened"),
                ("how-it-was-found", "How it was found"),
                ("impact", "Impact"),
                ("what-changed", "What changed"),
            ] {
                need(&mut out, name, &format!("missing ## {label}"));
            }
        }
        "item" if rec.body.trim().is_empty() => {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                0,
                "an item needs a paragraph that says what and why",
            ));
        }
        _ => {}
    }
    out
}

fn check_workflow(store: &Store, rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    if rec.legacy() {
        return out;
    }
    match rec.type_name() {
        "deploy" => {
            if rec.fm.get_str("authorized_by") != Some("author") {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    rec.field_line("authorized_by"),
                    "authorized_by must be author",
                ));
            }
            if rec.fm.get_str("word").unwrap_or("").is_empty() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "no 'word' — a deploy runs only on the author's word",
                ));
            }
            if rec.fm.get_str("word_date").map(util::valid_date) != Some(true)
                || rec.fm.get_str("word_date") == Some("unknown")
            {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    rec.field_line("word_date"),
                    "word_date is required",
                ));
            }
            if rec.fm.get_str("backup") == Some("none") {
                let bw = rec.fm.get_str("backup_word").unwrap_or("");
                if bw.is_empty() {
                    out.push(Problem::error(
                        "C12",
                        rec.rel.clone(),
                        0,
                        "backup none needs backup_word",
                    ));
                } else if bw.chars().count() > 300 {
                    out.push(Problem::error(
                        "C12",
                        rec.rel.clone(),
                        rec.field_line("backup_word"),
                        "backup_word is longer than 300 characters",
                    ));
                }
            }
            if rec.fm.strings("carries").is_empty() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "carries is required",
                ));
            }
            if rec.fm.strings("rollback").is_empty() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "rollback is required",
                ));
            }
            if rec.fm.get_str("target") == Some("web") {
                for key in ["commit", "binary_sha256", "migrations"] {
                    if rec.fm.get_str(key).unwrap_or("").is_empty() {
                        out.push(Problem::error(
                            "C12",
                            rec.rel.clone(),
                            0,
                            format!("{key} is required for a web deploy"),
                        ));
                    }
                }
            }
            if rec.fm.get_str("target") == Some("mobile") {
                for key in ["version", "version_code", "apk_sha256", "devices"] {
                    if rec.fm.strings(key).is_empty() && rec.fm.get_str(key).is_none() {
                        out.push(Problem::error(
                            "C12",
                            rec.rel.clone(),
                            0,
                            format!("{key} is required for a mobile deploy"),
                        ));
                    }
                }
            }
            if earlier_deploy(store, rec) && rec.fm.get_str("previous").is_none() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "previous is required when an earlier deploy of this target exists",
                ));
            }
        }
        "loop" if rec.status() == "done" => {
            let parts = record::parts_of(rec);
            if record::find_part(&parts, "not-verified").is_none() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "a done loop needs ## Not verified",
                ));
            }
            match rec.fm.get_str("review") {
                Some("owed") => {}
                Some(id) if store.get(id).is_some() => {}
                Some(id) => out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    rec.field_line("review"),
                    format!("review {id} does not exist"),
                )),
                None => out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "a done loop needs review (an id or owed)",
                )),
            }
            if rec.fm.get("gates").is_none() {
                out.push(Problem::error(
                    "C12",
                    rec.rel.clone(),
                    0,
                    "a done loop needs gates",
                ));
            }
        }
        "review" => {
            let parts = record::parts_of(rec);
            for part in parts.iter().filter(|p| finding_name(&p.name)) {
                if !finding_ok(&part.text) {
                    out.push(Problem::error(
                        "C12",
                        rec.rel.clone(),
                        part.line,
                        format!(
                            "{} needs **Severity:** and **Failing case:** or **Not fixed:**",
                            part.name
                        ),
                    ));
                }
            }
        }
        "open" if rec.status() == "closed" && rec.fm.get_str("closed_by").is_none() => {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                0,
                "a closed open item needs closed_by",
            ));
        }
        _ => {}
    }
    out.extend(check_supersedes(store, rec));
    out
}

fn finding_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.first() == Some(&b'F') && b.len() > 1 && b[1..].iter().all(|c| c.is_ascii_digit())
}

fn finding_ok(text: &str) -> bool {
    static SEV: OnceLock<Regex> = OnceLock::new();
    let sev =
        SEV.get_or_init(|| Regex::new(r"(?i)\*\*Severity:\*\*\s*(high|medium|low)\b").unwrap());
    sev.is_match(text) && (text.contains("**Failing case:**") || text.contains("**Not fixed:**"))
}

fn earlier_deploy(store: &Store, rec: &Record) -> bool {
    let target = rec.fm.get_str("target").unwrap_or("");
    let date = rec.date();
    store.records.iter().any(|other| {
        other.type_name() == "deploy"
            && other.id_str() != rec.id_str()
            && other.fm.get_str("target").unwrap_or("") == target
            && !other.date().is_empty()
            && other.date() != "unknown"
            && other.date() < date
    })
}

fn check_supersedes(store: &Store, rec: &Record) -> Vec<Problem> {
    let mut out = Vec::new();
    let Some(id) = rec.id() else { return out };
    let me = id.canonical();
    for other_id in rec.fm.strings("supersedes") {
        let Some(other) = store.get(&other_id) else {
            continue;
        };
        let back = other.fm.strings("superseded_by");
        if !back.iter().any(|s| canon(s) == me) {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                rec.field_line("supersedes"),
                format!("{me} supersedes {other_id}, which does not list it in superseded_by"),
            ));
        }
    }
    for other_id in rec.fm.strings("superseded_by") {
        let Some(other) = store.get(&other_id) else {
            continue;
        };
        let back = other.fm.strings("supersedes");
        if !back.iter().any(|s| canon(s) == me) {
            out.push(Problem::error(
                "C12",
                rec.rel.clone(),
                rec.field_line("superseded_by"),
                format!("{me} is superseded_by {other_id}, which does not list it in supersedes"),
            ));
        }
    }
    out
}

fn canon(s: &str) -> String {
    Id::parse(s)
        .map(|i| i.canonical())
        .unwrap_or_else(|| s.to_string())
}

fn check_reservations(store: &Store, want: &impl Fn(&str) -> bool) -> Vec<Problem> {
    let mut out = Vec::new();
    for kind in ["rule", "open"] {
        let blocks: Vec<_> = store
            .reservations
            .iter()
            .filter(|r| r.type_name == kind)
            .collect();
        for i in 0..blocks.len() {
            for j in (i + 1)..blocks.len() {
                let a = blocks[i];
                let b = blocks[j];
                if a.from <= b.to && b.from <= a.to {
                    out.push(Problem::error(
                        "C11",
                        "",
                        0,
                        format!(
                            "{kind} reservations overlap: {} {}-{} and {} {}-{}",
                            a.for_id, a.from, a.to, b.for_id, b.from, b.to
                        ),
                    ));
                }
            }
        }
    }
    // A block of this checkout that overlaps another loop's block in the shared ledger: two
    // worktrees took the same numbers (one of them before the ledger, or with an older
    // trailmark). A warning, because the ledger is this machine's and not part of the tree.
    if let Some(ledger) = &store.ledger {
        for mine in &store.reservations {
            for other in &ledger.entries {
                if other.type_name == mine.type_name
                    && canon(&other.for_id) != canon(&mine.for_id)
                    && mine.from <= other.to
                    && other.from <= mine.to
                {
                    let place = if other.worktree.is_empty() {
                        String::new()
                    } else {
                        format!(" (taken in {})", other.worktree)
                    };
                    out.push(Problem::warning(
                        "C21",
                        "",
                        0,
                        format!(
                            "{} reservation {} {}-{} overlaps {} {}-{} in the worktrees' shared ledger{place}; rebase and re-mint the later loop's numbers",
                            mine.type_name, mine.for_id, mine.from, mine.to, other.for_id, other.from, other.to
                        ),
                    ));
                }
            }
        }
    }
    for rec in &store.records {
        if rec.legacy() || !want(&rec.rel) {
            continue;
        }
        if rec.type_name() == "rule" {
            let Some(id) = rec.id() else { continue };
            let Some(n) = id::rule_number(&id.key) else {
                continue;
            };
            let source = canon(rec.fm.get_str("source").unwrap_or(""));
            let holders: Vec<_> = store
                .reservations
                .iter()
                .filter(|r| r.type_name == "rule" && r.from <= n && n <= r.to)
                .collect();
            let ok = holders.len() == 1 && canon(&holders[0].for_id) == source;
            if !ok {
                let inside = holders
                    .first()
                    .map(|r| canon(&r.for_id))
                    .unwrap_or_default();
                let message = if !inside.is_empty() && inside != source {
                    format!(
                        "{} minted by {source} inside {inside}'s block",
                        id.canonical()
                    )
                } else {
                    format!("{} is not inside a reservation of {source}", id.canonical())
                };
                out.push(Problem::error("C11", "", 0, message));
            }
        }
        if rec.type_name() == "open" {
            let Some(id) = rec.id() else { continue };
            let Ok(n) = id.key.parse::<u32>() else {
                continue;
            };
            let opened = canon(rec.fm.get_str("opened_by").unwrap_or(""));
            let holders: Vec<_> = store
                .reservations
                .iter()
                .filter(|r| r.type_name == "open" && r.from <= n && n <= r.to)
                .collect();
            let ok = holders.len() == 1 && canon(&holders[0].for_id) == opened;
            if !ok {
                let inside = holders
                    .first()
                    .map(|r| canon(&r.for_id))
                    .unwrap_or_default();
                let message = if !inside.is_empty() && inside != opened {
                    format!(
                        "{} minted by {opened} inside {inside}'s block",
                        id.canonical()
                    )
                } else {
                    format!("{} is not inside a reservation of {opened}", id.canonical())
                };
                out.push(Problem::error("C11", "", 0, message));
            }
        }
    }
    out
}

fn check_outside_rules(store: &Store) -> Vec<Problem> {
    let mut firsts = HashSet::new();
    for rec in store.records.iter().filter(|r| r.type_name() == "rule") {
        if let Some(line) = rec.body.lines().next() {
            firsts.insert(line.trim().to_string());
        }
    }
    let mut out = Vec::new();
    let store_root = store.config.store_dir(&store.root);
    walk_md_outside(&store.root, &store_root, &mut |path, text| {
        let rel = path
            .strip_prefix(&store.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let lines: Vec<&str> = text.lines().collect();
        let mut fenced = false;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let t = line.trim();
            // Rule-shaped means the heading AND a rule's body: an incident or a Rule block
            // before the next heading. A numbered section in a design note (`### 2.1 Setup`)
            // has the heading only. A person can still waive a line in waivers.toml.
            if rule_heading(t)
                && !firsts.contains(t)
                && has_rule_body(&lines[i + 1..])
                && !store.waived("C06", &rel, i + 1, "")
            {
                out.push(Problem::error(
                    "C06",
                    rel.clone(),
                    i + 1,
                    "rule-shaped heading outside the store",
                ));
            }
        }
    });
    out
}

fn has_rule_body(after: &[&str]) -> bool {
    for line in after {
        if line.starts_with('#') {
            return false;
        }
        let t = line.trim_start();
        if t.starts_with("> **Rule.") || t.starts_with("**Incident") {
            return true;
        }
    }
    false
}

fn rule_heading(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^### 2\.\d").unwrap());
    re.is_match(line)
}

fn walk_md_outside(dir: &Path, store_root: &Path, out: &mut impl FnMut(&Path, String)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if path.starts_with(store_root) {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist") {
            continue;
        }
        if path.is_dir() {
            walk_md_outside(&path, store_root, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            out(&path, text);
        }
    }
}

pub fn code_hits(store: &Store) -> Vec<Hit> {
    store.code_hits().as_ref().clone()
}

/// For a hit, the lines of every hit in the same file citing the same text: a waiver's line
/// is a hint among them (`Store::waived_hit`).
fn same_lines(hits: &[Hit]) -> impl Fn(&Hit) -> Vec<usize> + '_ {
    move |hit: &Hit| {
        let key = crate::store::compact(&hit.written);
        hits.iter()
            .filter(|h| h.path == hit.path && crate::store::compact(&h.written) == key)
            .map(|h| h.line)
            .collect()
    }
}

fn check_code(store: &Store, hits: &[Hit]) -> Vec<Problem> {
    let mut out = Vec::new();
    let same = same_lines(hits);
    for hit in hits {
        if store.waived_hit("C10", &hit.path, hit.line, &hit.written, &same(hit)) {
            continue;
        }
        if hit.ambiguous {
            out.push(Problem::error(
                "C10",
                hit.path.clone(),
                hit.line,
                format!("{} is ambiguous: {}", hit.written, hit.ids.join(", ")),
            ));
            continue;
        }
        if hit.ids.is_empty() || hit.ids.iter().any(|id| store.get(id).is_none()) {
            let mut message = format!("{} resolves to nothing", hit.written);
            if let Some(id) = hit
                .ids
                .first()
                .and_then(|s| Id::parse(s))
                .or_else(|| Id::parse(&hit.written))
            {
                let near = store.nearest(&id);
                if !near.is_empty() {
                    message.push_str(&format!(". Nearest existing: {}", near.join(", ")));
                }
            }
            out.push(Problem::error("C10", hit.path.clone(), hit.line, message));
        }
    }
    out
}

fn check_tombstone_cites(
    store: &Store,
    code: &[Hit],
    want: &impl Fn(&str) -> bool,
) -> Vec<Problem> {
    let mut out = Vec::new();
    let same = same_lines(code);
    for hit in code {
        for id in &hit.ids {
            if let Some(rec) = store.get(id) {
                if is_tombstone(rec.status())
                    && !store.waived_hit("C15", &hit.path, hit.line, &hit.written, &same(hit))
                {
                    out.push(tombstone_warning(&hit.path, hit.line, id, rec));
                }
            }
        }
    }
    for rec in &store.records {
        if rec.legacy() || !want(&rec.rel) {
            continue;
        }
        let hits: Vec<Hit> = scan::field_hits(store, rec)
            .into_iter()
            .chain(scan::body_hits(store, rec))
            .collect();
        // Every id named on each line: a line that names X and the record that superseded it
        // tells the story of the replacement; it is not a stale citation.
        let mut on_line: std::collections::BTreeMap<usize, BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for hit in &hits {
            on_line
                .entry(hit.line)
                .or_default()
                .extend(hit.ids.iter().cloned());
        }
        for hit in hits {
            if matches!(
                hit.field.as_deref(),
                Some(
                    "supersedes"
                        | "superseded_by"
                        | "narrows"
                        | "narrowed_by"
                        | "either"
                        | "follows"
                )
            ) {
                continue;
            }
            // The record that superseded X names X on purpose ("Supersedes: the chain is
            // the same"); only a citation from elsewhere is stale.
            let replaced = rec.fm.strings("supersedes");
            for id in &hit.ids {
                if let Some(target) = store.get(id) {
                    let told =
                        target.fm.strings("superseded_by").iter().any(|next| {
                            on_line.get(&hit.line).is_some_and(|ids| ids.contains(next))
                        });
                    if is_tombstone(target.status())
                        && !replaced.contains(&target.id_str())
                        && !told
                    {
                        out.push(tombstone_warning(&rec.rel, hit.line, id, target));
                    }
                }
            }
        }
    }
    let _ = is_sentinel;
    out
}

fn tombstone_warning(path: &str, line: usize, id: &str, rec: &Record) -> Problem {
    let mut message = format!("{path}:{line} cites {id} ({})", rec.status());
    let by = rec.fm.strings("superseded_by");
    if let Some(other) = by.first() {
        message = format!("{path}:{line} cites {id} (superseded by {other})");
    }
    Problem::warning("C15", "", 0, message)
}

fn check_fresh(store: &Store, planned: &[indexgen::Planned]) -> Vec<Problem> {
    let mut out = Vec::new();
    let dir = store.config.store_dir(&store.root).join("index");
    let mut expected = BTreeSet::new();
    for file in planned {
        expected.insert(file.rel.clone());
        let path = store.config.store_dir(&store.root).join(&file.rel);
        match std::fs::read(&path) {
            Ok(bytes) if bytes == file.bytes => {}
            Ok(_) => out.push(Problem::error(
                "C08",
                "",
                0,
                format!(
                    "{} is stale (it differs from what trailmark index writes) — run trailmark index",
                    file.rel
                ),
            )),
            Err(_) => out.push(Problem::error(
                "C08",
                "",
                0,
                format!("{} is stale (missing) — run trailmark index", file.rel),
            )),
        }
    }
    if dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let rel = format!("index/{name}");
                if !expected.contains(&rel) {
                    out.push(Problem::error(
                        "C08",
                        "",
                        0,
                        format!("{rel} is stale (not produced by trailmark index) — run trailmark index"),
                    ));
                }
            }
        }
    }
    out
}

/// `commit_index = false`: `index/` is a local cache. A file of it git still tracks is
/// committed by every loop that runs `trailmark index`, and every two of them conflict on it when
/// they land. A warning: the cache itself is fine.
fn check_index_untracked(store: &Store) -> Vec<Problem> {
    let index = format!("{}/index", store.config.store_rel.trim_end_matches('/'));
    let n = indexgen::tracked_index_files(store);
    if n == 0 {
        return Vec::new();
    }
    vec![Problem::warning(
        "C22",
        "",
        0,
        format!(
            "{n} files under {index}/ are tracked by git, but the indexes are a local cache (commit_index = false): git rm -r --cached -q {index} and commit"
        ),
    )]
}

fn check_history(store: &Store) -> Vec<Problem> {
    let mut out = Vec::new();
    if !gitinfo::own_repo(&store.root) {
        out.push(Problem::warning(
            "C13",
            "",
            0,
            "not a git repository; history was not checked",
        ));
        return out;
    }
    let spec = "COMMIT:%H".to_string();
    let log = gitinfo::output(
        &store.root,
        &[
            "log".into(),
            "-p".into(),
            "--all".into(),
            format!("--format={spec}"),
            "--".into(),
            store.config.store_rel.clone(),
        ],
    );
    let Some(log) = log else {
        return out;
    };
    let existing = store.existing_ids();
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut commit = String::new();
    for line in log.lines() {
        if let Some(rest) = line.strip_prefix("COMMIT:") {
            commit = rest.trim().to_string();
            continue;
        }
        let body = line
            .strip_prefix('+')
            .or_else(|| line.strip_prefix('-'))
            .unwrap_or(line);
        let Some(rest) = body.strip_prefix("id:") else {
            continue;
        };
        let value = rest.trim().trim_matches('"');
        if let Some(id) = Id::parse(value) {
            seen.entry(id.canonical()).or_insert_with(|| commit.clone());
        }
    }
    for (id, commit) in seen {
        if !existing.contains(&id) {
            let short = if commit.len() >= 8 {
                &commit[..8]
            } else {
                commit.as_str()
            };
            out.push(Problem::error(
                "C13",
                "",
                0,
                format!("{id} existed at {short} and has no file now"),
            ));
        }
    }
    out
}

fn check_hashes(store: &Store, want: &impl Fn(&str) -> bool) -> Vec<Problem> {
    let mut out = Vec::new();
    if !gitinfo::own_repo(&store.root) {
        return out;
    }
    static HASH: OnceLock<Regex> = OnceLock::new();
    let hash = HASH.get_or_init(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());
    for rec in &store.records {
        if !want(&rec.rel) {
            continue;
        }
        for key in ["commit", "commits", "commits_read"] {
            for value in rec.fm.strings(key) {
                if !hash.is_match(&value) {
                    continue;
                }
                if !gitinfo::has_commit(&store.root, &value) {
                    out.push(Problem::warning(
                        "C17",
                        rec.rel.clone(),
                        rec.field_line(key),
                        format!("commit {value} is not in this repository"),
                    ));
                }
            }
        }
    }
    out
}

fn check_stubs(store: &Store) -> Vec<Problem> {
    let mut out = Vec::new();
    let dir = store.config.store_dir(&store.root).join("_migration");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("manifest") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(source) = text.lines().find_map(|l| l.strip_prefix("source\t")) else {
            continue;
        };
        let file = store.root.join(source);
        let Ok(body) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines = body.lines().count();
        if lines > 40 {
            out.push(Problem::error(
                "C18",
                source,
                0,
                format!("{lines} lines; expected the stub"),
            ));
        }
    }
    out
}

fn changed_set(store: &Store) -> Option<HashSet<String>> {
    if !gitinfo::own_repo(&store.root) {
        return None;
    }
    let diff = gitinfo::output(
        &store.root,
        &["diff".into(), "--name-only".into(), "HEAD".into()],
    )?;
    let untracked = gitinfo::output(
        &store.root,
        &[
            "ls-files".into(),
            "--others".into(),
            "--exclude-standard".into(),
        ],
    )
    .unwrap_or_default();
    let mut names = HashSet::new();
    for line in diff.lines().chain(untracked.lines()) {
        names.insert(line.trim().trim_start_matches("./").to_string());
    }
    let prefix = format!("{}/", store.config.store_rel.trim_end_matches('/'));
    let mut rels = HashSet::new();
    for name in names {
        if let Some(rest) = name.strip_prefix(&prefix) {
            rels.insert(rest.to_string());
        }
    }
    Some(rels)
}

/// C12: one live deploy per target. A second `live` makes "what is production" a guess.
fn check_live_deploys(store: &Store) -> Vec<Problem> {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rec in &store.records {
        if rec.type_name() == "deploy" && rec.status() == "live" {
            by.entry(rec.fm.get_str("target").unwrap_or("").to_string())
                .or_default()
                .push(rec.id_str());
        }
    }
    let mut out = Vec::new();
    for (target, ids) in by {
        if ids.len() > 1 {
            out.push(Problem::error(
                "C12",
                "",
                0,
                format!(
                    "{} are all live for target {target}; one deploy is live, the others superseded",
                    ids.join(", ")
                ),
            ));
        }
    }
    out
}

fn check_relations(store: &Store) -> Vec<Problem> {
    let mut out = Vec::new();
    for rec in &store.records {
        let Some(id) = rec.id() else { continue };
        let me = id.canonical();
        if !rec.fm.strings("follows").is_empty()
            && !matches!(rec.type_name(), "rule" | "decision" | "item" | "loop")
        {
            out.push(Problem::error(
                "C19",
                rec.rel.clone(),
                rec.field_line("follows"),
                format!(
                    "{me} follows a convention, and a {} does not",
                    rec.type_name()
                ),
            ));
        }
        for key in ["narrows", "either", "narrowed_by", "follows"] {
            for raw in rec.fm.strings(key) {
                let other_id = relate_canon(&raw);
                if other_id == me {
                    out.push(Problem::error(
                        "C19",
                        rec.rel.clone(),
                        rec.field_line(key),
                        format!("{me} relates to itself"),
                    ));
                    continue;
                }
                let Some(other) = store.get(&other_id) else {
                    continue;
                };
                if matches!(key, "narrows" | "either" | "narrowed_by") {
                    if !crate::relate::convention(rec) || !crate::relate::convention(other) {
                        out.push(Problem::error(
                            "C19",
                            rec.rel.clone(),
                            rec.field_line(key),
                            format!("{key} connects a rule or a decision to a rule or a decision"),
                        ));
                    } else if !crate::relate::live(rec) || !crate::relate::live(other) {
                        let dead = if crate::relate::live(rec) { other } else { rec };
                        out.push(Problem::error(
                            "C19",
                            rec.rel.clone(),
                            rec.field_line(key),
                            format!(
                                "{} is {}. Both sides of {key} stay active",
                                dead.id_str(),
                                dead.status()
                            ),
                        ));
                    }
                }
                if key == "follows"
                    && (!crate::relate::convention(other) || !crate::relate::live(other))
                {
                    out.push(Problem::error(
                        "C19",
                        rec.rel.clone(),
                        rec.field_line("follows"),
                        format!(
                            "{me} follows {other_id}, which is {} {}",
                            other.status(),
                            other.type_name()
                        ),
                    ));
                }
                if key == "narrows"
                    && known_date(rec.date())
                    && known_date(other.date())
                    && rec.date() < other.date()
                {
                    out.push(Problem::warning(
                        "C19",
                        rec.rel.clone(),
                        rec.field_line("narrows"),
                        format!(
                            "{me} narrows {other_id} but is dated {}, before {}",
                            rec.date(),
                            other.date()
                        ),
                    ));
                }
            }
        }
        agree(&mut out, store, rec, &me, "narrows", "narrowed_by");
        agree(&mut out, store, rec, &me, "narrowed_by", "narrows");
        agree(&mut out, store, rec, &me, "either", "either");
        let live_narrowers = crate::relate::pointing(store, rec, "narrowed_by").len();
        if live_narrowers > crate::relate::NARROW_CAP {
            out.push(Problem::warning(
                "C19",
                rec.rel.clone(),
                rec.field_line("narrowed_by"),
                format!("{me} has {live_narrowers} narrowers; restate it and supersede it"),
            ));
        }
    }
    let mut pairs = BTreeSet::new();
    for rec in &store.records {
        let Some(id) = rec.id() else { continue };
        let me = id.canonical();
        for key in ["supersedes", "superseded_by", "narrows", "either"] {
            for raw in rec.fm.strings(key) {
                let other = relate_canon(&raw);
                if other == me {
                    continue;
                }
                let (a, b) = if me < other {
                    (me.clone(), other)
                } else {
                    (other, me.clone())
                };
                if pairs.insert((a.clone(), b.clone())) {
                    let kinds = crate::relate::kinds_between(store, &a, &b);
                    if kinds.len() > 1 {
                        out.push(Problem::error(
                            "C19",
                            "",
                            0,
                            format!("{a} and {b} use {}. A pair has one of supersedes, narrows, or either", kinds.join(" and ")),
                        ));
                    }
                }
            }
        }
    }
    out.extend(narrow_cycles(store));
    out
}

fn agree(
    out: &mut Vec<Problem>,
    store: &Store,
    rec: &Record,
    me: &str,
    forward: &str,
    back_key: &str,
) {
    for raw in rec.fm.strings(forward) {
        let other_id = relate_canon(&raw);
        let Some(other) = store.get(&other_id) else {
            continue;
        };
        let back = other.fm.strings(back_key);
        if !back.iter().any(|s| relate_canon(s) == me) {
            out.push(Problem::error(
                "C19",
                rec.rel.clone(),
                rec.field_line(forward),
                format!("{me} lists {other_id} in {forward}, which does not list it in {back_key}"),
            ));
        }
    }
}

fn narrow_cycles(store: &Store) -> Vec<Problem> {
    let edges = crate::relate::narrow_edges(store);
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let starts: Vec<String> = edges.keys().cloned().collect();
    for start in starts {
        walk_narrow(
            &start,
            &edges,
            &mut visiting,
            &mut visited,
            &mut stack,
            &mut out,
        );
    }
    out
}

fn walk_narrow(
    id: &str,
    edges: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    out: &mut Vec<Problem>,
) {
    if visited.contains(id) {
        return;
    }
    if !visiting.insert(id.to_string()) {
        if let Some(i) = stack.iter().position(|s| s == id) {
            let mut cyc = stack[i..].to_vec();
            cyc.push(id.to_string());
            out.push(Problem::error(
                "C19",
                "",
                0,
                format!("narrows cycle: {}", cyc.join(" → ")),
            ));
        }
        return;
    }
    stack.push(id.to_string());
    if let Some(nexts) = edges.get(id) {
        for next in nexts {
            walk_narrow(next, edges, visiting, visited, stack, out);
        }
    }
    stack.pop();
    visiting.remove(id);
    visited.insert(id.to_string());
}

fn known_date(date: &str) -> bool {
    date.len() >= 10 && date != "unknown" && date.as_bytes().get(4) == Some(&b'-')
}

fn relate_canon(s: &str) -> String {
    crate::relate::canon(s)
}
