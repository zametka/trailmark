//! Commands that write records. Validation runs in memory. A failure writes nothing.

use crate::check::{self, CheckOpts};
use crate::frontmatter::{Frontmatter, Value};
use crate::id::{self, Id};
use crate::record::Record;
use crate::store::{Reservation, Store};
use crate::util;
use std::path::Path;

pub struct Written {
    pub id: String,
    pub paths: Vec<String>,
}

pub enum Fail {
    Usage(String),
    Missing(String),
    Conflict(String),
    Invalid(String),
    Frozen(String),
}

const LIST_KEYS: &[&str] = &[
    "areas",
    "links",
    "platform",
    "gates",
    "learned",
    "opened",
    "carries",
    "rollback",
    "applies_to",
    "missed",
    "items",
    "decisions",
    "loops",
    "reviews",
    "commits",
    "commits_read",
    "supersedes",
    "superseded_by",
    "aliases",
    "narrows",
    "narrowed_by",
    "either",
    "follows",
    "closes",
    "deploys",
    "devices",
    "earned",
    "new_migrations",
];

pub fn template_for(type_name: &str) -> Option<&'static str> {
    Some(match type_name {
        "rule" => include_str!("../templates/rule.md"),
        "charter" => include_str!("../templates/charter.md"),
        "decision" => include_str!("../templates/decision.md"),
        "discussion" => include_str!("../templates/discussion.md"),
        "item" => include_str!("../templates/item.md"),
        "loop" => include_str!("../templates/loop.md"),
        "review" => include_str!("../templates/review.md"),
        "open" => include_str!("../templates/open.md"),
        "deploy" => include_str!("../templates/deploy.md"),
        "fact" => include_str!("../templates/fact.md"),
        "incident" => include_str!("../templates/incident.md"),
        _ => return None,
    })
}

pub fn sync(rec: &mut Record) {
    let rendered = rec.fm.render();
    rec.body_line = rendered.lines().count() + 1;
    let mut raw = rendered.into_bytes();
    raw.extend(rec.body.as_bytes());
    rec.raw = raw;
}

pub struct AddRequest {
    pub type_name: String,
    pub id: Option<String>,
    pub key: Option<String>,
    pub for_id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub areas: Vec<String>,
    pub fields: Vec<(String, String)>,
    pub parts: Vec<(String, String)>,
    pub item: Option<String>,
    pub platform: Vec<String>,
    /// `--figure "<path under the store> | <caption>"`, repeatable.
    pub figures: Vec<String>,
}

pub fn add(store: &Store, req: &AddRequest) -> Result<(Store, Written), Fail> {
    ensure_open(store)?;
    if template_for(&req.type_name).is_none() {
        return Err(Fail::Usage(format!("unknown type {}", req.type_name)));
    }
    // A review's title is its loop's: `add review --for LOOP-B1` is "Review of B1", the
    // form the alias patterns and the old documents already use.
    let default_title = match (req.type_name.as_str(), &req.for_id) {
        ("review", Some(for_id)) => for_id
            .strip_prefix("LOOP-")
            .map(|key| format!("Review of {key}")),
        _ => None,
    };
    let title = req
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or(default_title)
        .ok_or_else(|| Fail::Usage(format!("add {} needs --title", req.type_name)))?;
    one_line("--title", &title)?;
    for (key, value) in &req.fields {
        // `legacy: true` exempts a record from the body and workflow checks (C07, C11, C12);
        // only migration writes it. `id` and `type` come from the command, not from a field.
        if matches!(key.as_str(), "id" | "type" | "legacy") {
            return Err(Fail::Usage(format!("--field {key} is not set by add")));
        }
        one_line(&format!("--field {key}"), value)?;
    }
    for value in req
        .areas
        .iter()
        .chain(req.platform.iter())
        .chain(req.item.iter())
        .chain(req.figures.iter())
    {
        one_line("a list value", value)?;
    }
    let id = allocate_id(store, req)?;
    if let Some(taken) = store.get(&id.canonical()) {
        // A taken key used to send the next loop to a guessed key. A review's key is its
        // loop's: a taken review key is not a reason to mint a different loop.
        let holder = format!(" ({}, {})", one_line_title(taken.title()), taken.status());
        let hint = match next_free_key(store, &id) {
            Some(free) => format!("{holder}; the next free key is {free}"),
            None if id.prefix == "REV" => {
                format!(
                    "{holder}; a loop has one review: `trailmark show {}`",
                    id.canonical()
                )
            }
            None => String::new(),
        };
        return Err(Fail::Conflict(format!(
            "id {} already exists{hint}",
            id.canonical()
        )));
    }
    let mut fm = Frontmatter::default();
    fm.set("id", Value::String(id.canonical()));
    fm.set("type", Value::String(req.type_name.clone()));
    fm.set("title", Value::String(title.clone()));
    fm.set("date", Value::String(util::today_utc()));
    fm.set(
        "status",
        Value::String(default_status(&req.type_name).into()),
    );
    apply_type_defaults(store, req, &id, &mut fm)?;
    if !req.areas.is_empty() {
        fm.set("areas", list_value(&req.areas));
    }
    if let Some(item) = &req.item {
        fm.set("item", Value::String(item.clone()));
    }
    if !req.platform.is_empty() {
        fm.set("platform", list_value(&req.platform));
    }
    for (key, value) in &req.fields {
        fm.set(key, parse_field(key, value));
    }
    if !req.figures.is_empty() {
        fm.set("figures", list_value(&req.figures));
    }
    let body = build_body(
        &req.type_name,
        &id,
        &title,
        fm.get_str("date").unwrap_or(""),
        req,
    );
    let local = fm.get_str("scope") == Some("local");
    let rel = id::rel_path(&id, &req.type_name, local)
        .ok_or_else(|| Fail::Usage("cannot place this record".into()))?;
    let mut rec = Record {
        rel: rel.clone(),
        raw: Vec::new(),
        fm,
        body,
        body_line: 1,
    };
    sync(&mut rec);

    let mut next = store.clone();
    let mut reservations = Vec::new();
    let mut superseded: Vec<String> = Vec::new();
    if req.type_name == "loop" {
        reservations.extend(reserve_pair(&next, &id.canonical())?);
        next.reservations.extend(reservations.clone());
    }
    if req.type_name == "review" {
        // A review mints rules and opens items, like a loop.
        let blocks = reserve_pair(&next, &id.canonical())?;
        reservations.extend(blocks.clone());
        next.reservations.extend(blocks);
        if let Some(for_id) = &req.for_id {
            let want = canonical(for_id).unwrap_or_else(|| for_id.clone());
            if let Some(loop_rec) = next.records.iter_mut().find(|r| r.id_str() == want) {
                loop_rec.fm.set("review", Value::String(id.canonical()));
                sync(loop_rec);
            }
        }
    }
    if req.type_name == "deploy" && rec.status() == "live" {
        // The new deploy is production now: the one before it of the same target is
        // superseded, and is named as `previous` unless the command named one.
        let target = rec.fm.get_str("target").unwrap_or("").to_string();
        let mut earlier: Vec<String> = Vec::new();
        for other in next.records.iter_mut() {
            if other.type_name() == "deploy"
                && other.status() == "live"
                && other.fm.get_str("target").unwrap_or("") == target
            {
                earlier.push(other.id_str());
                other.fm.set("status", Value::String("superseded".into()));
                sync(other);
            }
        }
        if rec.fm.get("previous").is_none() {
            if let Some(prev) = earlier.last() {
                rec.fm.set("previous", Value::String(prev.clone()));
                sync(&mut rec);
            }
        }
        superseded = earlier;
    }
    if req.type_name == "rule" {
        if let Some(for_id) = &req.for_id {
            let want = canonical(for_id).unwrap_or_else(|| for_id.clone());
            if let Some(loop_rec) = next.records.iter_mut().find(|r| r.id_str() == want) {
                loop_rec.fm.push_unique("learned", &id.canonical());
                sync(loop_rec);
            }
        }
    }
    next.records.push(rec);
    next.records.sort_by(|a, b| a.rel.cmp(&b.rel));
    validate(store, &next)?;
    let mut paths = vec![format!("{}/{}", store.config.store_rel, rel)];
    for block in &reservations {
        paths.push(format!(
            "{}/{}",
            store.config.store_rel,
            crate::store::reservation_rel(&block.for_id)
        ));
    }
    if req.type_name == "rule" || req.type_name == "review" {
        if let Some(for_id) = &req.for_id {
            if let Some(other) = store.get(for_id) {
                paths.push(format!("{}/{}", store.config.store_rel, other.rel));
            }
        }
    }
    for id in &superseded {
        if let Some(other) = next.get(id) {
            paths.push(format!("{}/{}", store.config.store_rel, other.rel));
        }
    }
    paths.sort();
    paths.dedup();
    Ok((
        next,
        Written {
            id: id.canonical(),
            paths,
        },
    ))
}

pub fn commit_store(root: &Path, before: &Store, after: &Store) -> Result<Vec<String>, Fail> {
    let dir = before.config.store_dir(root);
    let mut wrote = Vec::new();
    for rec in &after.records {
        let old = before.records.iter().find(|r| r.rel == rec.rel);
        if old.map(|r| r.raw.as_slice()) == Some(rec.raw.as_slice()) {
            continue;
        }
        let path = dir.join(&rec.rel);
        util::write_atomic(&path, &rec.raw).map_err(|e| Fail::Invalid(e.to_string()))?;
        wrote.push(format!("{}/{}", before.config.store_rel, rec.rel));
    }
    if after.reservations.len() != before.reservations.len() {
        // Each block goes to its owner's own file (`reservations/LOOP-B1.toml`), which only
        // that loop's worktree writes, so two loops never edit one file.
        for block in after.reservations.iter().skip(before.reservations.len()) {
            let rel = crate::store::reservation_rel(&block.for_id);
            let path = dir.join(&rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Fail::Invalid(e.to_string()))?;
            }
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&render_reservation(block));
            util::write_atomic(&path, text.as_bytes()).map_err(|e| Fail::Invalid(e.to_string()))?;
            wrote.push(format!("{}/{rel}", before.config.store_rel));
        }
        // The shared ledger learns the new blocks, and any block of this checkout it has not
        // seen (one taken before the ledger existed, or by an older trailmark). The command
        // holds the ledger lock (cli::dispatch).
        if let Some(ledger) = &before.ledger {
            crate::ledger::append(ledger, root, &after.reservations).map_err(Fail::Invalid)?;
        }
    }
    wrote.sort();
    wrote.dedup();
    Ok(wrote)
}

pub fn link(
    store: &Store,
    from: &str,
    to: &str,
    rel: &str,
    remove: bool,
) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    let from_id = canonical(from).ok_or_else(|| Fail::Usage(format!("malformed id {from}")))?;
    // A file a record names is linked by its path from the repository root: a rule
    // and the file that broke it. It must exist, and only
    // `links` takes one.
    if canonical(to).is_none() && rel == "links" && to.contains('/') {
        return link_path(store, &from_id, to, remove);
    }
    let to_id = canonical(to).ok_or_else(|| Fail::Usage(format!("malformed id {to}")))?;
    if store.get(&from_id).is_none() {
        return Err(Fail::Missing(format!("unknown id {from_id}")));
    }
    if store.get(&to_id).is_none() {
        return Err(Fail::Missing(format!("unknown id {to_id}")));
    }
    let known = [
        "links",
        "source",
        "supersedes",
        "closes",
        "carries",
        "reviews",
        "learned",
        "opened",
        "item",
        "decisions",
        "deploy",
        "narrows",
        "either",
        "follows",
        "missed",
    ];
    if !known.contains(&rel) {
        return Err(Fail::Usage(format!("unknown relation {rel}")));
    }
    if !remove {
        if let Some(msg) = crate::relate::reject_link(store, &from_id, &to_id, rel) {
            return Err(Fail::Invalid(msg));
        }
    }
    let mut next = store.clone();
    if rel == "closes" && !remove {
        let target = next.records.iter().find(|r| r.id_str() == to_id).unwrap();
        if target.status() == "closed" {
            return Err(Fail::Invalid(format!("{to_id} is already closed")));
        }
    }
    {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == from_id)
            .unwrap();
        if remove {
            clear_link(&mut rec.fm, rel, &to_id);
        } else {
            put_link(&mut rec.fm, rel, &to_id);
        }
        sync(rec);
    }
    if rel == "supersedes" {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == to_id)
            .unwrap();
        if remove {
            rec.fm.remove_item("superseded_by", &from_id);
            if rec.status() == "superseded" && rec.fm.strings("superseded_by").is_empty() {
                rec.fm.set("status", Value::String("active".into()));
            }
        } else {
            rec.fm.push_unique("superseded_by", &from_id);
            rec.fm.set("status", Value::String("superseded".into()));
        }
        sync(rec);
    }
    if rel == "narrows" || rel == "either" {
        let inverse = if rel == "narrows" {
            "narrowed_by"
        } else {
            "either"
        };
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == to_id)
            .unwrap();
        if remove {
            rec.fm.remove_item(inverse, &from_id);
        } else {
            rec.fm.push_unique(inverse, &from_id);
        }
        sync(rec);
    }
    if rel == "closes" && !remove {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == to_id)
            .unwrap();
        rec.fm.set("status", Value::String("closed".into()));
        rec.fm.set("closed_by", Value::String(from_id.clone()));
        rec.fm.set("closed", Value::String(util::today_utc()));
        sync(rec);
    }
    if rel == "closes" && remove {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == to_id)
            .unwrap();
        if rec.fm.get_str("closed_by") == Some(from_id.as_str()) {
            rec.fm.set("status", Value::String("open".into()));
            rec.fm
                .keys
                .retain(|(k, _)| k != "closed_by" && k != "closed");
        }
        sync(rec);
    }
    validate(store, &next)?;
    let mut paths = Vec::new();
    for id in [&from_id, &to_id] {
        if let Some(rec) = next.get(id) {
            let old = store.get(id);
            if old.map(|r| r.raw.as_slice()) != Some(rec.raw.as_slice()) {
                paths.push(format!("{}/{}", store.config.store_rel, rec.rel));
            }
        }
    }
    Ok((next, paths))
}

fn link_path(
    store: &Store,
    from_id: &str,
    path: &str,
    remove: bool,
) -> Result<(Store, Vec<String>), Fail> {
    let clean = path.trim_start_matches("./");
    if clean.starts_with('/') || clean.split('/').any(|p| p == "..") {
        return Err(Fail::Usage(format!(
            "{path}: a linked file is a path from the repository root"
        )));
    }
    let file = clean.split('#').next().unwrap_or(clean);
    if !remove && !store.root.join(file).exists() {
        return Err(Fail::Missing(format!("{file} does not exist in the tree")));
    }
    let mut next = store.clone();
    let rec = next
        .records
        .iter_mut()
        .find(|r| r.id_str() == from_id)
        .ok_or_else(|| Fail::Missing(format!("unknown id {from_id}")))?;
    if remove {
        clear_link(&mut rec.fm, "links", clean);
    } else {
        put_link(&mut rec.fm, "links", clean);
    }
    sync(rec);
    let rel = rec.rel.clone();
    validate(store, &next)?;
    Ok((next, vec![format!("{}/{}", store.config.store_rel, rel)]))
}

pub fn retire(
    store: &Store,
    id: &str,
    why: &str,
    by: Option<&str>,
) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    if why.trim().is_empty() {
        return Err(Fail::Usage("retire needs --why".into()));
    }
    one_line("--why", why)?;
    let id = canonical(id).ok_or_else(|| Fail::Usage("malformed id".into()))?;
    if store.get(&id).is_none() {
        return Err(Fail::Missing(format!("unknown id {id}")));
    }
    let status = store.get(&id).unwrap().status();
    if status == "retired" || status == "superseded" {
        return Err(Fail::Invalid(format!("{id} is already {status}")));
    }
    let mut next = store.clone();
    {
        let rec = next.records.iter_mut().find(|r| r.id_str() == id).unwrap();
        rec.fm.set("retired_note", Value::String(why.to_string()));
        if by.is_none() {
            rec.fm.set("status", Value::String("retired".into()));
        }
        sync(rec);
    }
    let mut paths = vec![format!(
        "{}/{}",
        store.config.store_rel,
        next.get(&id).unwrap().rel
    )];
    if let Some(by_id) = by {
        let by_id = canonical(by_id).ok_or_else(|| Fail::Usage(format!("malformed id {by_id}")))?;
        if next.get(&by_id).is_none() {
            return Err(Fail::Missing(format!("unknown id {by_id}")));
        }
        let (linked, more) = link(&next, &by_id, &id, "supersedes", false)?;
        next = linked;
        paths.extend(more);
    }
    validate(store, &next)?;
    Ok((next, paths))
}

/// `trailmark set <id> status=<value> [waiting_on=<x>] [order=<n>] --why "<line>" [--by <id>]`
/// the status of an item or an open item changes after it is written —
/// an item runs, is built, waits, is dropped; an open item closes or is not fixed — and
/// before this only `link --rel closes` could change one. The body is never rewritten:
/// `wontfix` appends a `## Why not` part. A loop or a review is closed by `close`; a rule,
/// decision or fact leaves by `retire` or `link --rel supersedes`.
pub fn set(
    store: &Store,
    id: &str,
    pairs: &[(String, String)],
    why: &str,
    by: Option<&str>,
) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    let id = canonical(id).ok_or_else(|| Fail::Usage(format!("malformed id {id}")))?;
    let Some(old) = store.get(&id) else {
        return Err(Fail::Missing(format!("unknown id {id}")));
    };
    let type_name = old.type_name().to_string();
    match type_name.as_str() {
        "item" | "open" => {}
        "loop" | "review" => {
            return Err(Fail::Usage(format!(
                "{id} is a {type_name}: it is closed by trailmark close, not set"
            )))
        }
        _ => {
            return Err(Fail::Usage(format!(
                "{id} is a {type_name}: it leaves by trailmark retire {id} --why \"…\", or trailmark link <new> {id} --rel supersedes"
            )))
        }
    }
    if why.trim().is_empty() {
        return Err(Fail::Usage("set needs --why \"<one line>\"".into()));
    }
    one_line("--why", why)?;
    let allowed_keys: &[&str] = if type_name == "item" {
        &["status", "waiting_on", "order"]
    } else {
        &["status"]
    };
    let mut status: Option<String> = None;
    let mut waiting_on: Option<String> = None;
    let mut order: Option<String> = None;
    for (key, value) in pairs {
        if !allowed_keys.contains(&key.as_str()) {
            return Err(Fail::Usage(format!(
                "set {key}: an {type_name} takes {}",
                allowed_keys.join(", ")
            )));
        }
        let value = value.trim().to_string();
        match key.as_str() {
            "status" => status = Some(value),
            "waiting_on" => waiting_on = Some(value),
            _ => {
                if !is_int(&value) {
                    return Err(Fail::Usage(format!("order must be a number, not {value}")));
                }
                order = Some(value);
            }
        }
    }
    let before = old.status().to_string();
    let after = status.clone().unwrap_or_else(|| before.clone());
    let allowed: Vec<&str> = crate::check::statuses(&type_name)
        .into_iter()
        .filter(|s| !crate::record::is_tombstone(s))
        .collect();
    if !allowed.contains(&after.as_str()) {
        return Err(Fail::Invalid(format!(
            "status '{after}' is not one of {} for an {type_name}",
            allowed.join(", ")
        )));
    }
    // The same status again, or no status at all, is a note: a correction to the last why
    //. It changes
    // nothing but the note, so `closed_by`, `closed` and `waiting_on` stay as they were.
    let note_only = after == before && waiting_on.is_none() && order.is_none();
    let by_id = match by {
        None => None,
        Some(b) if crate::record::is_sentinel(b) => Some(b.to_string()),
        Some(b) => {
            let c = canonical(b).ok_or_else(|| Fail::Usage(format!("malformed id {b}")))?;
            if store.get(&c).is_none() {
                return Err(Fail::Missing(format!("unknown id {c}")));
            }
            Some(c)
        }
    };
    if type_name == "open" && after == "closed" && by_id.is_none() && !note_only {
        return Err(Fail::Usage(format!(
            "closing {id} needs --by <the loop, review or deploy that closed it, or author>"
        )));
    }
    let mut next = store.clone();
    let rel = {
        let rec = next.records.iter_mut().find(|r| r.id_str() == id).unwrap();
        rec.fm.set("status", Value::String(after.clone()));
        if note_only {
            // Nothing below: the status and its fields are as they were.
        } else if type_name == "item" {
            if after == "waiting" {
                let on = waiting_on
                    .clone()
                    .or_else(|| rec.fm.get_str("waiting_on").map(|s| s.to_string()))
                    .or_else(|| by_id.clone());
                match on {
                    Some(on) => rec.fm.set("waiting_on", Value::String(on)),
                    None => {
                        return Err(Fail::Usage(format!(
                            "{id} waiting needs waiting_on=<author or an id>"
                        )))
                    }
                }
            } else {
                if waiting_on.is_some() {
                    return Err(Fail::Usage("waiting_on goes with status=waiting".into()));
                }
                rec.fm.keys.retain(|(k, _)| k != "waiting_on");
            }
            if let Some(n) = &order {
                rec.fm.set("order", parse_field("order", n));
            }
            if let Some(b) = &by_id {
                if b.starts_with("LOOP-") {
                    rec.fm.push_unique("loops", b);
                }
                if b.starts_with("DEP-") && after == "done" {
                    rec.fm.set("deploy", Value::String(b.clone()));
                }
            }
        }
        if type_name == "open" && !note_only {
            match after.as_str() {
                "closed" => {
                    rec.fm.set(
                        "closed_by",
                        Value::String(by_id.clone().unwrap_or_default()),
                    );
                    rec.fm.set("closed", Value::String(util::today_utc()));
                }
                "wontfix" => {
                    rec.fm
                        .keys
                        .retain(|(k, _)| k != "closed_by" && k != "closed");
                    if !crate::record::parts_of(rec)
                        .iter()
                        .any(|p| p.name == "why-not")
                    {
                        if !rec.body.ends_with('\n') {
                            rec.body.push('\n');
                        }
                        rec.body.push_str(&format!("\n## Why not\n{why}\n"));
                    }
                }
                _ => rec
                    .fm
                    .keys
                    .retain(|(k, _)| k != "closed_by" && k != "closed"),
            }
        }
        let mut note = format!("{} {before} → {after}: {why}", util::today_utc());
        if before == after {
            note = format!("{} note: {why}", util::today_utc());
        }
        if let Some(b) = &by_id {
            note.push_str(&format!(" (by {b})"));
        }
        // A record is not edited after it is added, and a status's why is part of it: the
        // note a later `set` replaces moves to `status_history`, oldest first, so a second
        // change keeps the first one's reason.
        if let Some(earlier) = rec.fm.get_str("status_note").map(|s| s.to_string()) {
            let mut history = rec.fm.strings("status_history");
            history.push(earlier);
            rec.fm.set(
                "status_history",
                Value::List(history.into_iter().map(Value::String).collect()),
            );
        }
        rec.fm.set("status_note", Value::String(note));
        sync(rec);
        rec.rel.clone()
    };
    validate(store, &next)?;
    Ok((next, vec![format!("{}/{rel}", store.config.store_rel)]))
}

/// Add or remove a figure on an existing record (`trailmark figure <id> --add "<path> | <caption>"`).
/// `trailmark tag <id> --add a,b --drop c`: the situations a rule, fact or decision must
/// surface for, in `applies_to`. `context` gives a rule whose tag the task names a bonus,
/// and prints a recipe whose tag it names. A tag is a lowercase word or words joined by
/// hyphens: an area, a platform, or a situation (`worktree`, `release-stack`, `touch`).
pub fn tag(
    store: &Store,
    id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    let id = canonical(id).ok_or_else(|| Fail::Usage("malformed id".into()))?;
    let Some(rec) = store.get(&id) else {
        return Err(Fail::Missing(format!("unknown id {id}")));
    };
    if !matches!(rec.type_name(), "rule" | "fact" | "decision" | "loop") {
        return Err(Fail::Usage(format!(
            "tag takes a rule, a fact, a decision or a loop; {id} is a {}",
            rec.type_name()
        )));
    }
    if add.is_empty() && remove.is_empty() {
        return Err(Fail::Usage("tag needs --add or --drop".into()));
    }
    for t in add {
        if !valid_tag(t) {
            return Err(Fail::Usage(format!(
                "tag '{t}': lowercase letters, digits and hyphens, at most 24"
            )));
        }
    }
    let mut next = store.clone();
    {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == id)
            .ok_or_else(|| Fail::Missing(format!("unknown id {id}")))?;
        let mut items = rec.fm.strings("applies_to");
        items.retain(|t| !remove.contains(t));
        for t in add {
            if !items.contains(t) {
                items.push(t.clone());
            }
        }
        if items.is_empty() {
            rec.fm.keys.retain(|(k, _)| k != "applies_to");
        } else {
            rec.fm.set("applies_to", list_value(&items));
        }
        sync(rec);
    }
    validate(store, &next)?;
    let rel = next.get(&id).map(|r| r.rel.clone()).unwrap_or_default();
    Ok((next, vec![format!("{}/{rel}", store.config.store_rel)]))
}

pub fn valid_tag(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 24
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !t.starts_with('-')
        && !t.ends_with('-')
}

pub fn figure(
    store: &Store,
    id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    let id = canonical(id).ok_or_else(|| Fail::Usage("malformed id".into()))?;
    if store.get(&id).is_none() {
        return Err(Fail::Missing(format!("unknown id {id}")));
    }
    if add.is_empty() && remove.is_empty() {
        return Err(Fail::Usage("figure needs --add or --drop".into()));
    }
    for value in add.iter().chain(remove.iter()) {
        one_line("a figure", value)?;
    }
    let mut next = store.clone();
    {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == id)
            .ok_or_else(|| Fail::Missing(format!("unknown id {id}")))?;
        let mut items = rec.fm.strings("figures");
        for path in remove {
            let path = path.trim();
            items.retain(|raw| crate::record::split_figure(raw).0 != path);
        }
        for raw in add {
            let (path, caption) = crate::record::split_figure(raw);
            items.retain(|old| crate::record::split_figure(old).0 != path);
            items.push(format!("{path} | {caption}"));
        }
        if items.is_empty() {
            rec.fm.keys.retain(|(k, _)| k != "figures");
        } else {
            rec.fm.set("figures", list_value(&items));
        }
        sync(rec);
    }
    validate(store, &next)?;
    let rel = next.get(&id).map(|r| r.rel.clone()).unwrap_or_default();
    Ok((next, vec![format!("{}/{rel}", store.config.store_rel)]))
}

/// `trailmark close`: the end of a loop or a review, written by the tool. The read guard keeps an
/// agent from opening a record file, so "edit docs/records/loops/X.md" was a step no agent
/// under the guard could take (a cold read stopped there).
pub struct CloseRequest {
    pub id: String,
    pub gates: Vec<String>,
    pub review: Option<String>,
    pub verdict: Option<String>,
    pub parts: Vec<(String, String)>,
    pub fields: Vec<(String, String)>,
}

pub fn close(store: &Store, req: &CloseRequest) -> Result<(Store, Vec<String>), Fail> {
    ensure_open(store)?;
    let id = canonical(&req.id).ok_or_else(|| Fail::Usage(format!("malformed id {}", req.id)))?;
    let Some(old) = store.get(&id) else {
        return Err(Fail::Missing(format!("unknown id {id}")));
    };
    let kind = old.type_name().to_string();
    if kind != "loop" && kind != "review" {
        return Err(Fail::Usage(format!(
            "close takes a loop or a review; {id} is a {kind}"
        )));
    }
    for g in &req.gates {
        one_line("--gates", g)?;
    }
    for (key, value) in &req.fields {
        if matches!(
            key.as_str(),
            "id" | "type" | "legacy" | "status" | "gates" | "review" | "verdict"
        ) {
            return Err(Fail::Usage(format!(
                "--field {key} is not set this way; use the flag of that name"
            )));
        }
        one_line(&format!("--field {key}"), value)?;
    }
    let mut next = store.clone();
    let rel;
    {
        let rec = next
            .records
            .iter_mut()
            .find(|r| r.id_str() == id)
            .ok_or_else(|| Fail::Missing(format!("unknown id {id}")))?;
        let names: Vec<String> = crate::record::parts_of(rec)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for (name, text) in &req.parts {
            let want = util::slug(name);
            if !names.contains(&want) {
                return Err(Fail::Usage(format!(
                    "{id} has no part `{name}`; its parts: {}",
                    names.join(", ")
                )));
            }
            rec.body = replace_section(&rec.body, name, text.trim_end());
        }
        let gates = if req.gates.is_empty() {
            rec.fm.strings("gates")
        } else {
            req.gates.clone()
        };
        if gates.is_empty() {
            return Err(Fail::Invalid(format!(
                "{id} closes with its gates: --gates \"cargo test 290/0\" --gates \"clippy clean\" (one flag per gate, each the command's own count)"
            )));
        }
        rec.fm.set("gates", list_value(&gates));
        if kind == "loop" {
            if let Some(r) = &req.review {
                let r = if r == "owed" {
                    r.clone()
                } else {
                    canonical(r).unwrap_or_else(|| r.clone())
                };
                rec.fm.set("review", Value::String(r));
            }
            if rec.fm.get_str("review").is_none() {
                return Err(Fail::Invalid(format!(
                    "{id} closes with --review REV-<KEY> (its review) or --review owed"
                )));
            }
            if req.verdict.is_some() {
                return Err(Fail::Usage("--verdict is for a review".into()));
            }
        } else {
            if let Some(v) = &req.verdict {
                if !matches!(v.as_str(), "go" | "go-with-fixes" | "hold" | "no-go") {
                    return Err(Fail::Usage(
                        "--verdict is one of go, go-with-fixes, hold, no-go".into(),
                    ));
                }
                rec.fm.set("verdict", Value::String(v.clone()));
            }
            if rec.fm.get_str("verdict").is_none() {
                return Err(Fail::Invalid(format!(
                    "{id} closes with --verdict go|go-with-fixes|hold|no-go"
                )));
            }
            if req.review.is_some() {
                return Err(Fail::Usage("--review is for a loop".into()));
            }
        }
        for (key, value) in &req.fields {
            rec.fm.set(key, parse_field(key, value));
        }
        rec.fm.set("status", Value::String("done".into()));
        let pending: Vec<String> = crate::record::parts_of(rec)
            .into_iter()
            .filter(|p| {
                // A part's text starts with its own heading.
                let rest: Vec<&str> = p.text.lines().skip(1).collect();
                rest.join("\n").trim() == "Pending."
            })
            .map(|p| p.name)
            .collect();
        if !pending.is_empty() {
            return Err(Fail::Invalid(format!(
                "{id} still says Pending. in: {} — pass --part <name>=<file> for each",
                pending.join(", ")
            )));
        }
        sync(rec);
        rel = rec.rel.clone();
    }
    validate(store, &next)?;
    Ok((next, vec![format!("{}/{rel}", store.config.store_rel)]))
}

pub fn reserve(
    store: &Store,
    type_name: &str,
    n: u32,
    for_id: &str,
) -> Result<(Store, Reservation), Fail> {
    ensure_open(store)?;
    if type_name != "rule" && type_name != "open" {
        return Err(Fail::Usage("reserve type must be rule or open".into()));
    }
    if n == 0 {
        return Err(Fail::Usage("reserve count must be at least 1".into()));
    }
    let for_canon = if for_id == "orchestrator" {
        "orchestrator".to_string()
    } else {
        canonical(for_id).ok_or_else(|| Fail::Usage(format!("malformed id {for_id}")))?
    };
    if for_canon != "orchestrator" && store.get(&for_canon).is_none() {
        return Err(Fail::Missing(format!("unknown id {for_canon}")));
    }
    let block = reserve_block(store, type_name, &for_canon, n)?;
    let mut next = store.clone();
    next.reservations.push(block.clone());
    Ok((next, block))
}

fn ensure_open(store: &Store) -> Result<(), Fail> {
    if store.config.frozen {
        Err(Fail::Frozen("store is frozen".into()))
    } else {
        Ok(())
    }
}

/// Refuse a write that adds an error. Errors the store already had (a legacy dead link, a
/// heading in another document) are not this write's, and they must not block every `add`
/// until someone repairs history. Problems are compared without their line, because adding
/// a field to a record moves the lines below it.
fn validate(before: &Store, after: &Store) -> Result<(), Fail> {
    after.invalidate();
    let opts = || CheckOpts {
        changed: false,
        history: false,
        strict: false,
        indexes: false,
        code: false,
    };
    let errors = |store: &Store| {
        check::check_store(store, &opts())
            .problems
            .into_iter()
            .filter(|p| p.severity == crate::issue::Severity::Error)
            .collect::<Vec<_>>()
    };
    let mut known: std::collections::BTreeMap<(String, String, String), usize> =
        std::collections::BTreeMap::new();
    for p in errors(before) {
        *known
            .entry((p.code.to_string(), p.path.clone(), p.message.clone()))
            .or_default() += 1;
    }
    let mut fresh = Vec::new();
    for p in errors(after) {
        let key = (p.code.to_string(), p.path.clone(), p.message.clone());
        match known.get_mut(&key) {
            Some(n) if *n > 0 => *n -= 1,
            _ => fresh.push(p),
        }
    }
    if fresh.is_empty() {
        Ok(())
    } else {
        let text = fresh
            .iter()
            .map(|p| p.render())
            .collect::<Vec<_>>()
            .join("\n");
        Err(Fail::Invalid(text))
    }
}

/// A value written into one frontmatter line. A newline would end the line and let the
/// value write other keys, or close the frontmatter early.
fn one_line(what: &str, value: &str) -> Result<(), Fail> {
    if value.chars().any(|c| c.is_control()) {
        return Err(Fail::Usage(format!(
            "{what} must be one line with no control characters"
        )));
    }
    Ok(())
}

fn allocate_id(store: &Store, req: &AddRequest) -> Result<Id, Fail> {
    if let Some(raw) = &req.id {
        let id = Id::parse(raw).ok_or_else(|| Fail::Usage(format!("malformed id {raw}")))?;
        let keyed = matches!(
            req.type_name.as_str(),
            "item" | "loop" | "review" | "deploy" | "fact" | "charter"
        );
        if !keyed {
            return Err(Fail::Usage(format!(
                "--id is not used for {}",
                req.type_name
            )));
        }
        if !id::type_prefix_ok(&req.type_name, &id.prefix) {
            return Err(Fail::Usage(format!(
                "{} is not a {} id",
                id.canonical(),
                req.type_name
            )));
        }
        return Ok(id);
    }
    let scheme = id::scheme();
    match req.type_name.as_str() {
        "rule" => {
            let for_id = req
                .for_id
                .clone()
                .ok_or_else(|| Fail::Usage("add rule needs --for LOOP-ID".into()))?;
            let for_id =
                canonical(&for_id).ok_or_else(|| Fail::Usage(format!("malformed id {for_id}")))?;
            let n = next_in_block(store, "rule", &for_id)?;
            let key = if scheme.section_rules() {
                format!("2.{n}")
            } else {
                format!("{n}")
            };
            Ok(Id {
                prefix: scheme.rule,
                key,
            })
        }
        "open" => {
            let for_id = req
                .for_id
                .clone()
                .ok_or_else(|| Fail::Usage("add open needs --for LOOP-ID".into()))?;
            let for_id =
                canonical(&for_id).ok_or_else(|| Fail::Usage(format!("malformed id {for_id}")))?;
            let n = next_in_block(store, "open", &for_id)?;
            Ok(Id {
                prefix: scheme.open,
                key: format!("{n:04}"),
            })
        }
        "loop" => code_id(&scheme.loop_prefix, req),
        "review" => {
            if let Some(for_id) = &req.for_id {
                let for_id = canonical(for_id)
                    .ok_or_else(|| Fail::Usage(format!("malformed id {for_id}")))?;
                let loop_id = Id::parse(&for_id)
                    .ok_or_else(|| Fail::Usage(format!("malformed id {for_id}")))?;
                if loop_id.prefix != scheme.loop_prefix {
                    return Err(Fail::Usage(format!(
                        "add review --for expects a {} id",
                        scheme.loop_prefix
                    )));
                }
                return Ok(Id {
                    prefix: scheme.review,
                    key: loop_id.key,
                });
            }
            code_id(&scheme.review, req)
        }
        "item" => code_id(&scheme.item, req),
        "deploy" => code_id(&scheme.deploy, req),
        "fact" => {
            let key = req
                .key
                .clone()
                .ok_or_else(|| Fail::Usage("add fact needs a key".into()))?;
            Ok(Id {
                prefix: scheme.fact.clone(),
                key: id::Id::parse(&format!("{}-{key}", scheme.fact))
                    .map(|i| i.key)
                    .unwrap_or(key),
            })
        }
        "charter" => {
            let key = req
                .key
                .clone()
                .or_else(|| req.id.clone())
                .ok_or_else(|| Fail::Usage("add charter needs a key".into()))?;
            let id = if key.contains('-') {
                Id::parse(&key)
            } else {
                Id::parse(&format!("{}-{key}", scheme.charter))
            };
            id.ok_or_else(|| Fail::Usage(format!("malformed charter key {key}")))
        }
        "decision" => Ok(Id {
            prefix: scheme.decision.clone(),
            key: format!("{:04}", next_seq(store, &scheme.decision, "decision")),
        }),
        "discussion" => Ok(Id {
            prefix: scheme.discussion.clone(),
            key: format!("{:04}", next_seq(store, &scheme.discussion, "discussion")),
        }),
        "incident" => Ok(Id {
            prefix: scheme.incident.clone(),
            key: format!("{:04}", next_seq(store, &scheme.incident, "incident")),
        }),
        other => Err(Fail::Usage(format!("unknown type {other}"))),
    }
}

fn one_line_title(t: &str) -> String {
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 60 {
        format!("{}…", t.chars().take(59).collect::<String>())
    } else {
        t
    }
}

/// For a loop key that is taken (`B1`), the next key with the same letters that no record,
/// and no block in this checkout or the worktrees' ledger, holds (`B3` when `B2` is already
/// taken). A review takes its loop's key, so it gets none.
fn next_free_key(store: &Store, id: &Id) -> Option<String> {
    let scheme = id::scheme();
    if id.prefix != scheme.loop_prefix {
        return None;
    }
    let letters: String = id
        .key
        .chars()
        .take_while(|c| c.is_ascii_uppercase())
        .collect();
    let digits: String = id.key[letters.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let start: u32 = digits.parse().ok()?;
    let held = |key: &str| {
        let scheme = id::scheme();
        let loop_id = format!("{}-{key}", scheme.loop_prefix);
        let rev_id = format!("{}-{key}", scheme.review);
        store.get(&loop_id).is_some()
            || store.get(&rev_id).is_some()
            || store
                .all_reservations()
                .any(|r| r.for_id == loop_id || r.for_id == rev_id)
    };
    (start + 1..start + 200)
        .map(|n| format!("{letters}{n}"))
        .find(|k| !held(k))
}

fn code_id(prefix: &str, req: &AddRequest) -> Result<Id, Fail> {
    let key = req
        .key
        .clone()
        .ok_or_else(|| Fail::Usage(format!("add {} needs a key", req.type_name)))?;
    let raw = if key.contains('-') {
        key
    } else {
        format!("{prefix}-{key}")
    };
    Id::parse(&raw)
        .filter(|id| id.prefix == prefix)
        .ok_or_else(|| Fail::Usage(format!("malformed {prefix} key")))
}

fn next_seq(store: &Store, prefix: &str, type_name: &str) -> u32 {
    let here = store
        .records
        .iter()
        .filter_map(|r| r.id())
        .filter(|i| i.prefix == prefix)
        .filter_map(|i| i.key.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let next_door = id::folder_for(type_name)
        .map(|folder| crate::ledger::peer_high_seq(&store.root, &store.config.store_rel, folder))
        .unwrap_or(0);
    here.max(next_door) + 1
}

fn next_in_block(store: &Store, kind: &str, for_id: &str) -> Result<u32, Fail> {
    let blocks: Vec<&Reservation> = store
        .reservations
        .iter()
        .filter(|r| {
            r.type_name == kind
                && canonical(&r.for_id).unwrap_or_else(|| r.for_id.clone()) == for_id
        })
        .collect();
    let Some(block) = blocks.last() else {
        return Err(Fail::Conflict(reserve_hint(store, kind, for_id)));
    };
    let used = used_numbers(store, kind);
    for n in block.from..=block.to {
        if !used.contains(&n) {
            return Ok(n);
        }
    }
    Err(Fail::Conflict(reserve_hint(store, kind, for_id)))
}

fn reserve_hint(store: &Store, kind: &str, for_id: &str) -> String {
    let n = if kind == "open" {
        store.config.reserve_open
    } else {
        store.config.reserve_rule
    };
    if store.get(for_id).is_none() {
        // The usual cause on a first day: the loop was never written (a cold reader).
        let key = for_id.strip_prefix("LOOP-").unwrap_or(for_id);
        return format!(
            "{for_id} does not exist; write it first: `trailmark add loop {key} --title \"…\" --item none --platform docs`"
        );
    }
    format!("ask the orchestrator for `trailmark reserve {kind} {n} --for {for_id}`")
}

fn used_numbers(store: &Store, kind: &str) -> std::collections::BTreeSet<u32> {
    let mut used = std::collections::BTreeSet::new();
    for rec in &store.records {
        let Some(id) = rec.id() else { continue };
        let n = if kind == "rule" {
            id::rule_number(&id.key)
        } else if id.prefix == "OPEN" {
            id.key.parse().ok()
        } else {
            None
        };
        if let Some(n) = n {
            if (kind == "rule" && rec.type_name() == "rule")
                || (kind == "open" && rec.type_name() == "open")
            {
                used.insert(n);
            }
        }
    }
    used
}

fn high_water(store: &Store, kind: &str) -> u32 {
    // The ledger counts: a block another worktree took and has not merged yet is taken.
    let from_res = store
        .all_reservations()
        .filter(|r| r.type_name == kind)
        .map(|r| r.to)
        .max()
        .unwrap_or(0);
    from_res.max(used_numbers(store, kind).iter().copied().max().unwrap_or(0))
}

fn reserve_pair(store: &Store, for_id: &str) -> Result<Vec<Reservation>, Fail> {
    let rules = reserve_block(store, "rule", for_id, store.config.reserve_rule)?;
    let mut phantom = store.clone();
    phantom.reservations.push(rules.clone());
    let opens = reserve_block(&phantom, "open", for_id, store.config.reserve_open)?;
    Ok(vec![rules, opens])
}

fn reserve_block(store: &Store, kind: &str, for_id: &str, n: u32) -> Result<Reservation, Fail> {
    let start = high_water(store, kind) + 1;
    let end = start + n - 1;
    for block in store.all_reservations().filter(|r| r.type_name == kind) {
        if start <= block.to && block.from <= end {
            return Err(Fail::Conflict(format!(
                "{kind} reservation {start}-{end} overlaps {} {}-{}",
                block.for_id, block.from, block.to
            )));
        }
    }
    let used = used_numbers(store, kind);
    for k in start..=end {
        if used.contains(&k) {
            return Err(Fail::Conflict(format!(
                "{kind} number {k} is already in use"
            )));
        }
    }
    Ok(Reservation {
        type_name: kind.into(),
        from: start,
        to: end,
        for_id: for_id.into(),
        date: util::today_utc(),
        worktree: String::new(),
    })
}

fn render_reservation(block: &Reservation) -> String {
    format!(
        "[[reservation]]\ntype = \"{}\"\nfrom = {}\nto = {}\nfor = \"{}\"\ndate = \"{}\"\n\n",
        block.type_name, block.from, block.to, block.for_id, block.date
    )
}

fn apply_type_defaults(
    store: &Store,
    req: &AddRequest,
    id: &Id,
    fm: &mut Frontmatter,
) -> Result<(), Fail> {
    match req.type_name.as_str() {
        "rule" => {
            if let Some(for_id) = &req.for_id {
                let for_id = canonical(for_id).unwrap_or_else(|| for_id.clone());
                fm.set("source", Value::String(for_id));
            }
        }
        "open" => {
            if let Some(for_id) = &req.for_id {
                let for_id = canonical(for_id).unwrap_or_else(|| for_id.clone());
                fm.set("opened_by", Value::String(for_id));
            }
        }
        "review" => {
            if let Some(for_id) = &req.for_id {
                let for_id = canonical(for_id).unwrap_or_else(|| for_id.clone());
                if store.get(&for_id).is_none() {
                    return Err(Fail::Missing(format!("unknown id {for_id}")));
                }
                fm.set("reviews", list_value(&[for_id]));
            }
        }
        "loop" => {
            fm.set("started", Value::String(util::today_utc()));
        }
        _ => {}
    }
    let _ = id;
    Ok(())
}

fn default_status(type_name: &str) -> &'static str {
    match type_name {
        "discussion" => "open",
        "item" => "queued",
        "loop" | "review" => "running",
        "open" | "incident" => "open",
        "deploy" => "live",
        _ => "active",
    }
}

fn build_body(type_name: &str, id: &Id, title: &str, date: &str, req: &AddRequest) -> String {
    if let Some(body) = &req.body {
        return if body.ends_with('\n') {
            body.clone()
        } else {
            format!("{body}\n")
        };
    }
    let mut text = template_for(type_name).unwrap_or("").to_string();
    text = text.replace("{{title}}", title);
    text = text.replace("{{date}}", date);
    text = text.replace("{{id}}", &id.canonical());
    for (name, content) in &req.parts {
        let token = format!("{{{{{name}}}}}");
        let cleaned = content.trim_end();
        if text.contains(&token) {
            text = text.replace(&token, cleaned);
        } else {
            text = replace_section(&text, name, cleaned);
        }
    }
    while let Some(start) = text.find("{{") {
        let Some(end) = text[start..].find("}}") else {
            break;
        };
        text.replace_range(start..start + end + 2, "Pending.");
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn replace_section(text: &str, name: &str, content: &str) -> String {
    let want = util::slug(name);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut replaced = false;
    while i < lines.len() {
        if let Some(rest) = lines[i].strip_prefix("## ") {
            if util::slug(rest) == want {
                out.push_str(lines[i]);
                out.push('\n');
                out.push_str(content);
                out.push('\n');
                i += 1;
                while i < lines.len() && !lines[i].starts_with("## ") {
                    i += 1;
                }
                replaced = true;
                continue;
            }
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    if !replaced {
        out.push_str(content);
        out.push('\n');
    }
    out
}

fn parse_field(key: &str, value: &str) -> Value {
    if value == "true" {
        return Value::Bool(true);
    }
    if value == "false" {
        return Value::Bool(false);
    }
    if LIST_KEYS.contains(&key) {
        let items: Vec<Value> = value
            .split(',')
            .map(|s| Value::String(s.trim().to_string()))
            .filter(|v| v.as_str() != Some(""))
            .collect();
        return Value::List(items);
    }
    if (key == "order" || key == "version_code") && is_int(value) {
        return Value::Int(value.parse().unwrap_or(0));
    }
    Value::String(value.to_string())
}

fn is_int(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && (!digits.starts_with('0') || digits == "0")
}

fn list_value(items: &[String]) -> Value {
    Value::List(items.iter().cloned().map(Value::String).collect())
}

fn put_link(fm: &mut Frontmatter, rel: &str, to: &str) {
    match rel {
        "source" | "item" | "deploy" => fm.set(rel, Value::String(to.into())),
        _ => fm.push_unique(rel, to),
    }
}

fn clear_link(fm: &mut Frontmatter, rel: &str, to: &str) {
    match rel {
        "source" | "item" | "deploy" => {
            if fm.get_str(rel) == Some(to) {
                fm.keys.retain(|(k, _)| k != rel);
            }
        }
        _ => fm.remove_item(rel, to),
    }
}

fn canonical(id: &str) -> Option<String> {
    Id::parse(id).map(|i| i.canonical())
}
