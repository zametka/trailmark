//! How two live conventions relate.
//!
//! `narrows` — a later record rewrites part of an earlier one. Both stay active.
//! Where they disagree, the later one wins, and the rest of the earlier one stands.
//! `either` — both were accepted and they disagree. A feature may follow either, and says which.
//! `follows` — the feature records the one it chose. The other accepted one stays visible.
//! `supersedes` still retires a whole record. These three do not.

use crate::id::Id;
use crate::record::{self, is_tombstone, Record};
use crate::search;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet};

pub const NARROW_CAP: usize = 5;

pub struct Group {
    pub full: String,
    pub line: String,
    /// Other ids whose text is already inside `full`.
    pub also: Vec<String>,
}

pub struct Block {
    pub id: String,
    pub text: String,
    pub also: Vec<String>,
}

pub fn convention(rec: &Record) -> bool {
    matches!(rec.type_name(), "rule" | "decision")
}

pub fn live(rec: &Record) -> bool {
    !is_tombstone(rec.status()) && rec.status() != "revoked" && rec.status() != "rolled-back"
}

pub fn normative(rec: &Record) -> String {
    let parts = record::parts_of(rec);
    if let Some(part) = record::find_part(&parts, "rule") {
        return with_newline(&part.text);
    }
    if let Some(part) = record::find_part(&parts, "decision") {
        return with_newline(&part.text);
    }
    format!("{}\n", rec.title())
}

/// The normative part followed by the record's figures, one line each.
pub fn normative_in(store: &Store, rec: &Record) -> String {
    format!("{}{}", normative(rec), figure_lines(store, rec))
}

/// `Figure: <repo path> — <caption>` per figure. The picture is never embedded: a harness
/// that can read images opens the path; any other still has the caption.
pub fn figure_lines(store: &Store, rec: &Record) -> String {
    let mut out = String::new();
    for raw in rec.fm.strings("figures") {
        let (path, caption) = record::split_figure(&raw);
        out.push_str(&format!(
            "Figure: {}/{path} — {caption}\n",
            store.config.store_rel.trim_end_matches('/')
        ));
    }
    out
}

pub fn trailer(store: &Store, rec: &Record) -> String {
    let mut body = figure_lines(store, rec);
    if !body.is_empty() {
        body.push('\n');
    }
    let narrowers = newest(pointing(store, rec, "narrowed_by"));
    if narrowers.len() > NARROW_CAP {
        body.push_str(&format!(
            "{} has {} narrowers; showing the {NARROW_CAP} newest.\n\n",
            rec.id_str(),
            narrowers.len()
        ));
    }
    for other in narrowers.iter().take(NARROW_CAP) {
        body.push_str(&format!(
            "Narrowed by {}{}. Where they disagree, {} wins. The rest of {} still stands.\n\n",
            other.id_str(),
            dated(other),
            other.id_str(),
            rec.id_str()
        ));
        body.push_str(&normative_in(store, other));
        body.push('\n');
    }
    for other in by_id(pointing(store, rec, "narrows")) {
        body.push_str(&format!(
            "Narrows {}{}. Where they disagree, {} wins. The rest of {} still stands.\n\n",
            other.id_str(),
            dated(other),
            rec.id_str(),
            other.id_str()
        ));
        body.push_str(&normative_in(store, other));
        body.push('\n');
    }
    for other in by_id(pointing(store, rec, "either")) {
        body.push_str(&format!(
            "Either {} is also accepted{}. They disagree. Follow either, and say which.\n\n",
            other.id_str(),
            dated(other)
        ));
        body.push_str(&normative_in(store, other));
        body.push('\n');
    }
    let followers = followers_of(store, &rec.id_str());
    if !followers.is_empty() {
        let names = join_ids(&followers);
        body.push_str(&format!("Followed by {names}.\n\n"));
    }
    if let Some(block) = followed_block(store, rec) {
        body.push_str(&block.text);
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("\n{body}")
    }
}

pub fn group(store: &Store, rec: &Record) -> Group {
    let narrowers = newest(pointing(store, rec, "narrowed_by"));
    let bases = by_id(pointing(store, rec, "narrows"));
    let peers = by_id(pointing(store, rec, "either"));
    let mut also = Vec::new();
    let mut full = format!(
        "{}  {}\n{}",
        rec.id_str(),
        label(rec),
        normative_in(store, rec)
    );
    if narrowers.len() > NARROW_CAP {
        full.push_str(&format!(
            "{} has {} narrowers; showing the {NARROW_CAP} newest.\n\n",
            rec.id_str(),
            narrowers.len()
        ));
    }
    for other in narrowers.iter().take(NARROW_CAP) {
        also.push(other.id_str());
        full.push_str(&format!(
            "Narrowed by {}{}. Where they disagree, {} wins. The rest of {} still stands.\n\n",
            other.id_str(),
            dated(other),
            other.id_str(),
            rec.id_str()
        ));
        full.push_str(&normative_in(store, other));
        full.push('\n');
    }
    for other in &bases {
        also.push(other.id_str());
        full.push_str(&format!(
            "Narrows {}{}. Where they disagree, {} wins. The rest of {} still stands.\n\n",
            other.id_str(),
            dated(other),
            rec.id_str(),
            other.id_str()
        ));
        full.push_str(&normative_in(store, other));
        full.push('\n');
    }
    for other in &peers {
        also.push(other.id_str());
        full.push_str(&format!(
            "Either {} is also accepted{}. They disagree. Follow either, and say which.\n\n",
            other.id_str(),
            dated(other)
        ));
        full.push_str(&normative_in(store, other));
        full.push('\n');
    }
    // The bases may themselves be in an either pair. One hop, so the rewrite does not hide it.
    for base in &bases {
        for peer in by_id(pointing(store, base, "either")) {
            if peer.id_str() == rec.id_str() || also.iter().any(|id| id == &peer.id_str()) {
                continue;
            }
            also.push(peer.id_str());
            full.push_str(&format!(
                "Either {} is also accepted{}. They disagree. Follow either, and say which.\n\n",
                peer.id_str(),
                dated(peer)
            ));
            full.push_str(&normative_in(store, peer));
            full.push('\n');
        }
    }
    also.sort();
    also.dedup();
    let line = format!(
        "{}  {}{}",
        rec.id_str(),
        label(rec),
        line_suffix(&narrowers, &bases, &peers)
    );
    Group { full, line, also }
}

pub fn followed_block(store: &Store, rec: &Record) -> Option<Block> {
    let chosen = by_id(pointing(store, rec, "follows"));
    if chosen.is_empty() {
        return None;
    }
    let mut also = Vec::new();
    let mut text = format!("Followed {}.\n", join_ids(&chosen));
    let mut peers = Vec::new();
    for item in &chosen {
        for peer in by_id(pointing(store, item, "either")) {
            if !peers.iter().any(|p: &&Record| p.id_str() == peer.id_str()) {
                peers.push(peer);
            }
        }
    }
    if !peers.is_empty() {
        text.push_str(&format!(
            "Also accepted: {}. They disagree. Follow either, and say which.\n",
            join_ids(&peers)
        ));
    }
    text.push('\n');
    for item in &chosen {
        also.push(item.id_str());
        text.push_str(&format!("{}\n{}", item.id_str(), normative_in(store, item)));
        text.push('\n');
    }
    for peer in &peers {
        also.push(peer.id_str());
        text.push_str(&format!("{}\n{}", peer.id_str(), normative_in(store, peer)));
        text.push('\n');
    }
    also.sort();
    also.dedup();
    Some(Block {
        id: chosen[0].id_str(),
        text,
        also,
    })
}

pub fn why_lines(store: &Store, rec: &Record) -> String {
    let mut out = String::new();
    let narrowers = newest(pointing(store, rec, "narrowed_by"));
    for other in narrowers.iter().take(NARROW_CAP) {
        out.push_str(&format!(
            "    narrowed by {} — {}\n",
            other.id_str(),
            other.title()
        ));
    }
    if narrowers.len() > NARROW_CAP {
        out.push_str(&format!(
            "    {} more narrowers\n",
            narrowers.len() - NARROW_CAP
        ));
    }
    for other in by_id(pointing(store, rec, "narrows")) {
        out.push_str(&format!(
            "    narrows {} — the rest of it still stands\n",
            other.id_str()
        ));
    }
    for other in by_id(pointing(store, rec, "either")) {
        out.push_str(&format!(
            "    either {} — both accepted; they disagree\n",
            other.id_str()
        ));
    }
    for other in by_id(pointing(store, rec, "follows")) {
        out.push_str(&format!(
            "    followed {} — {}\n",
            other.id_str(),
            other.title()
        ));
        for peer in by_id(pointing(store, other, "either")) {
            out.push_str(&format!(
                "    also accepted {} — {}\n",
                peer.id_str(),
                peer.title()
            ));
        }
    }
    let followers = followers_of(store, &rec.id_str());
    if !followers.is_empty() {
        out.push_str(&format!("    followed by {}\n", join_ids(&followers)));
    }
    out
}

pub fn suggestions(store: &Store, rec: &Record) -> Vec<String> {
    if !convention(rec) {
        return Vec::new();
    }
    let mine = notable(rec);
    if mine.len() < 4 {
        return Vec::new();
    }
    let mut hits: Vec<(usize, String)> = Vec::new();
    for other in &store.records {
        if other.id_str() == rec.id_str() || !convention(other) || !live(other) {
            continue;
        }
        if !areas_touch(rec, other) {
            continue;
        }
        if already_related(rec, other) {
            continue;
        }
        let theirs = notable(other);
        let shared = mine.intersection(&theirs).count();
        let smaller = mine.len().min(theirs.len());
        if shared >= 4 && shared * 2 >= smaller {
            hits.push((shared, other.id_str()));
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.truncate(3);
    hits.into_iter()
        .map(|(_, id)| {
            format!(
                "{me} shares words with {id}. If this rewrites part of it: trailmark link {me} {id} --rel narrows. If both are accepted and they disagree: trailmark link {me} {id} --rel either.",
                me = rec.id_str()
            )
        })
        .collect()
}

pub fn reject_link(store: &Store, from: &str, to: &str, rel: &str) -> Option<String> {
    if from == to {
        return Some("a record cannot relate to itself".into());
    }
    let from_rec = store.get(from)?;
    let to_rec = store.get(to)?;
    match rel {
        "narrows" | "either" => {
            if !convention(from_rec) || !convention(to_rec) {
                return Some(
                    "narrows and either connect a rule or a decision to a rule or a decision"
                        .into(),
                );
            }
            if let Some(msg) = dead_side(from_rec, rel) {
                return Some(msg);
            }
            if let Some(msg) = dead_side(to_rec, rel) {
                return Some(msg);
            }
            clash(store, from, to, rel)
        }
        "follows" => {
            if !follower(from_rec) || !convention(to_rec) {
                return Some(
                    "follows records a rule, decision, item, or loop choosing a rule or a decision"
                        .into(),
                );
            }
            if !live(to_rec) {
                Some(format!(
                    "{} is {}. Follow an active rule or decision",
                    to_rec.id_str(),
                    to_rec.status()
                ))
            } else {
                None
            }
        }
        "supersedes" => {
            let kinds = kinds_between(store, from, to);
            if kinds.iter().any(|k| *k == "narrows" || *k == "either") {
                Some(format!("{from} and {to} already use narrows or either. Supersedes retires a whole record"))
            } else {
                None
            }
        }
        // A loop or a review records a rule its bundle did not surface and it needed (its
        // recorded misses); `context` then gives that rule a bonus for tasks like it.
        "missed" => {
            if !matches!(from_rec.type_name(), "loop" | "review") {
                Some("missed is recorded on a loop or a review".into())
            } else if !matches!(to_rec.type_name(), "rule" | "fact" | "decision" | "loop") {
                // An earlier loop's report is what a later loop goes looking for.
                // `context` lists it under the target as "Earlier work".
                Some("missed names a rule, a fact, a decision or an earlier loop".into())
            } else if !live(to_rec) {
                Some(format!(
                    "{} is {}. Name the record that replaced it",
                    to_rec.id_str(),
                    to_rec.status()
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn dead_side(rec: &Record, rel: &str) -> Option<String> {
    if live(rec) {
        None
    } else {
        Some(format!(
            "{} is {}. Both sides of {rel} stay active",
            rec.id_str(),
            rec.status()
        ))
    }
}

fn clash(store: &Store, from: &str, to: &str, rel: &str) -> Option<String> {
    let kinds = kinds_between(store, from, to);
    if rel == "narrows" && kinds.contains(&"either") {
        return Some(format!(
            "{from} and {to} already use either. either means neither one wins"
        ));
    }
    if rel == "narrows" && kinds.contains(&"supersedes") {
        return Some(format!(
            "{from} and {to} already use supersedes. Narrowing keeps both records active"
        ));
    }
    if rel == "either" && kinds.contains(&"narrows") {
        return Some(format!(
            "{from} and {to} already use narrows. either means neither one wins"
        ));
    }
    if rel == "either" && kinds.contains(&"supersedes") {
        return Some(format!(
            "{from} and {to} already use supersedes. either keeps both records active"
        ));
    }
    None
}

pub fn kinds_between(store: &Store, a: &str, b: &str) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if points(store, a, b, "supersedes")
        || points(store, b, a, "supersedes")
        || points(store, a, b, "superseded_by")
        || points(store, b, a, "superseded_by")
    {
        kinds.push("supersedes");
    }
    if points(store, a, b, "narrows") || points(store, b, a, "narrows") {
        kinds.push("narrows");
    }
    if points(store, a, b, "either") || points(store, b, a, "either") {
        kinds.push("either");
    }
    kinds
}

pub fn pointing<'a>(store: &'a Store, rec: &Record, key: &str) -> Vec<&'a Record> {
    let mut out = Vec::new();
    for id in rec.fm.strings(key) {
        let id = canon(&id);
        if let Some(other) = store.get(&id) {
            if other.id_str() != rec.id_str() && live(other) {
                out.push(other);
            }
        }
    }
    out
}

pub fn followers_of<'a>(store: &'a Store, id: &str) -> Vec<&'a Record> {
    let mut out: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| {
            r.fm.strings("follows").iter().any(|s| canon(s) == id) && !is_tombstone(r.status())
        })
        .collect();
    out.sort_by_key(|a| a.id_str());
    if out.len() > 8 {
        out.truncate(8);
    }
    out
}

fn follower(rec: &Record) -> bool {
    matches!(rec.type_name(), "rule" | "decision" | "item" | "loop")
}

fn points(store: &Store, from: &str, to: &str, key: &str) -> bool {
    store
        .get(from)
        .map(|r| r.fm.strings(key).iter().any(|s| canon(s) == to))
        .unwrap_or(false)
}

fn already_related(a: &Record, b: &Record) -> bool {
    let id = b.id_str();
    [
        "narrows",
        "narrowed_by",
        "either",
        "supersedes",
        "superseded_by",
        "follows",
    ]
    .iter()
    .any(|key| a.fm.strings(key).iter().any(|s| canon(s) == id))
}

fn areas_touch(a: &Record, b: &Record) -> bool {
    let aa = a.areas();
    let bb = b.areas();
    if aa.is_empty() || bb.is_empty() {
        return true;
    }
    aa.iter().any(|x| bb.iter().any(|y| x == y))
}

fn notable(rec: &Record) -> BTreeSet<String> {
    let mut text = format!("{} {}", rec.title(), rec.fm.get_str("short").unwrap_or(""));
    text.push(' ');
    text.push_str(&normative(rec));
    search::tokenize(&text)
        .into_iter()
        .filter(|t| keep_token(t))
        .collect()
}

fn keep_token(tok: &str) -> bool {
    tok.len() >= 5 && !STOP.contains(&tok)
}

const STOP: &[&str] = &[
    "about", "after", "again", "being", "before", "between", "could", "every", "first", "incident",
    "other", "pending", "rule", "should", "still", "their", "there", "these", "those", "under",
    "until", "where", "which", "while", "would",
];

fn line_suffix(narrowers: &[&Record], bases: &[&Record], peers: &[&Record]) -> String {
    let mut bits = Vec::new();
    if !narrowers.is_empty() {
        let names = join_ids(
            &narrowers
                .iter()
                .take(NARROW_CAP)
                .copied()
                .collect::<Vec<_>>(),
        );
        bits.push(format!("narrowed by {names}"));
    }
    if !bases.is_empty() {
        bits.push(format!("narrows {}", join_ids(bases)));
    }
    if !peers.is_empty() {
        bits.push(format!("either {}", join_ids(peers)));
    }
    if bits.is_empty() {
        String::new()
    } else {
        format!(" ({})", bits.join("; "))
    }
}

fn newest(mut items: Vec<&Record>) -> Vec<&Record> {
    items.sort_by(|a, b| {
        date_order(a)
            .cmp(&date_order(b))
            .then(a.id_str().cmp(&b.id_str()))
    });
    items
}

fn date_order(rec: &Record) -> (u8, std::cmp::Reverse<String>) {
    if known_date(rec.date()) {
        (0, std::cmp::Reverse(rec.date().to_string()))
    } else {
        (1, std::cmp::Reverse(String::new()))
    }
}

fn by_id(mut items: Vec<&Record>) -> Vec<&Record> {
    items.sort_by_key(|a| a.id_str());
    items
}

fn known_date(date: &str) -> bool {
    date.len() >= 10 && date != "unknown" && date.as_bytes().get(4) == Some(&b'-')
}

fn dated(rec: &Record) -> String {
    if known_date(rec.date()) {
        format!(" ({})", rec.date())
    } else {
        String::new()
    }
}

fn label(rec: &Record) -> String {
    match rec.fm.get_str("short") {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => rec.title().to_string(),
    }
}

fn join_ids(items: &[&Record]) -> String {
    items
        .iter()
        .map(|r| r.id_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn with_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

pub fn canon(s: &str) -> String {
    Id::parse(s)
        .map(|i| i.canonical())
        .unwrap_or_else(|| s.to_string())
}

/// Edges of `narrows`, used by the cycle check. Keyed by canonical id.
pub fn narrow_edges(store: &Store) -> BTreeMap<String, Vec<String>> {
    let mut edges = BTreeMap::new();
    for rec in &store.records {
        let Some(id) = rec.id() else { continue };
        let me = id.canonical();
        let nexts: Vec<String> = rec
            .fm
            .strings("narrows")
            .into_iter()
            .map(|s| canon(&s))
            .filter(|s| s != &me)
            .collect();
        if !nexts.is_empty() {
            edges.insert(me, nexts);
        }
    }
    edges
}
