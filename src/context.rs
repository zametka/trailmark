//! A context bundle. Plain search, fixed shares, no embeddings.

use crate::id::Id;
use crate::indexgen::{self, Over};
use crate::record::{self, is_tombstone, Record};
use crate::search::{self, bm25};
use crate::store::Store;
use crate::util::{self, tok4};
use std::collections::BTreeSet;

pub struct Request {
    pub task: String,
    pub loop_id: Option<String>,
    pub review_id: Option<String>,
    pub item_id: Option<String>,
    pub core_only: bool,
    pub budget: usize,
    pub areas: Vec<String>,
    pub explain: bool,
    /// `--terms`: the term index's words the agent rewrote its task into.
    pub terms: Option<String>,
}

pub struct Bundle {
    pub markdown: String,
    pub tok: usize,
    pub budget: usize,
    pub included: Vec<String>,
    pub omitted: Vec<String>,
}

pub enum Fail {
    Usage(String),
    Missing(String),
    Over(Over),
}

struct Chunk {
    id: String,
    text: String,
    also: Vec<String>,
}

fn chunk(id: impl Into<String>, text: impl Into<String>) -> Chunk {
    Chunk {
        id: id.into(),
        text: text.into(),
        also: Vec::new(),
    }
}

pub fn build(store: &Store, req: &Request) -> Result<Bundle, Fail> {
    if req.budget > store.config.budgets.context_max {
        return Err(Fail::Usage(format!(
            "budget {} is above the maximum {}",
            req.budget, store.config.budgets.context_max
        )));
    }
    let cites = indexgen::cites(store);
    let core = with_elsewhere(
        indexgen::core_text(store, &cites).map_err(Fail::Over)?,
        &crate::ledger::running_elsewhere(store),
    );
    let core_tok = tok4(&core);
    if req.core_only {
        if core_tok > req.budget {
            return Err(Fail::Over(Over {
                rel: "index/core.md".into(),
                tok: core_tok,
                budget: req.budget,
            }));
        }
        return Ok(Bundle {
            markdown: core,
            tok: core_tok,
            budget: req.budget,
            included: vec!["core".into()],
            omitted: Vec::new(),
        });
    }

    let (label, mut query, areas, platforms) = resolve_target(store, req)?;
    // The term index: the terms named lead to their records, and their names are words
    // of the task from here on (the "normalized text" the author asked the search to read).
    let termed = match &req.terms {
        Some(given) => {
            let all = crate::terms::load(store).map_err(Fail::Usage)?;
            crate::terms::resolve(store, &all, given)
        }
        None => crate::terms::Resolved::default(),
    };
    if !termed.names.is_empty() {
        query.push('\n');
        query.push_str(&termed.names.join(". "));
        query.push('\n');
    }
    let mut own: Vec<String> = [&req.loop_id, &req.review_id]
        .into_iter()
        .flatten()
        .map(|id| {
            Id::parse(id)
                .map(|i| i.canonical())
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    // A loop and its review are one piece of work: the review's bundle is ranked on the loop's
    // brief, so the loop's own misses matched it at 0.95 and came back.
    for id in own.clone() {
        if let Some(rec) = store.get(&id) {
            own.extend(rec.fm.strings("reviews"));
        }
        if let Some(key) = id.strip_prefix("LOOP-") {
            own.push(format!("REV-{key}"));
        }
    }
    let (ranked, weak) = rank_rules(store, &query, &areas, &own, &cites, &termed.pointed);
    // What the core already lists by title (the principles, the most-cited rules) is not
    // listed again; it may still be printed in full, and be a seed of the related records.
    let in_core: BTreeSet<String> = crate::scan::id_regex()
        .find_iter(&core)
        .map(|m| m.as_str().to_string())
        .collect();

    let mut target = target_chunks(store, req, &label);
    if let Some(line) = crate::terms::target_line(&termed) {
        target.insert(target.len().min(1), chunk("terms", line));
    }
    target.extend(earlier_work(store, &query, &own));
    let mut opens = open_chunks(store, &areas);
    // A loop's or a review's own bundle is work in a worktree that lands on main, whatever its
    // brief says — and a new loop's brief is still `Pending.`.
    let in_worktree = req.loop_id.is_some() || req.review_id.is_some();
    let mut recipes = recipe_chunks(
        store,
        &query,
        &areas,
        &platforms,
        in_worktree,
        &termed.pointed,
    );
    let (mut facts, facts_all) = fact_chunks(store, &query, &areas, &platforms, &termed.pointed);
    let mut full = Vec::new();
    let mut lines = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for r in &ranked {
        if !seen.insert(r.id.clone()) {
            continue;
        }
        let Some(rec) = store.get(&r.id) else {
            continue;
        };
        let group = crate::relate::group(store, rec);
        for member in &group.also {
            seen.insert(member.clone());
        }
        // In full: the first `FULL_CAP` conventions (the full text took a third of
        // the bundle for one hit in fifteen; the one-line list found five).
        if full.len() < FULL_CAP && !weak && rec.type_name() != "fact" {
            let text = if record::find_part(&record::parts_of(rec), "rule").is_none()
                && record::find_part(&record::parts_of(rec), "decision").is_none()
            {
                // A migrated memory has no rule or decision part: its "How to apply" is.
                format!("{}  {}\n{}\n", r.id, one(rec.title()), how_line(&rec.body))
            } else {
                // A decision's part carries its own `## Decision` heading: not a bundle section.
                group
                    .full
                    .lines()
                    .filter(|l| !l.starts_with("## "))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            };
            full.push(Chunk {
                id: r.id.clone(),
                text,
                also: group.also.clone(),
            });
        }
        // The one-line list goes on while a record holds some of the task's words, a tag it
        // says, or a miss of a task like it. Past that floor the line is padding.
        if in_core.contains(&r.id)
            || (r.lexical < LINE_FLOOR
                && r.tagged.is_empty()
                && r.missed.is_none()
                && r.termed.is_empty()
                && !rec.pinned())
        {
            continue;
        }
        lines.push(Chunk {
            id: r.id.clone(),
            text: cut_line(&group.line, LINE_CHARS),
            also: group.also,
        });
    }

    // One hop from the best matches: the ids around the task's convention.
    let mut related = Vec::new();
    if !weak {
        let top = ranked.first().map(|r| r.score).unwrap_or(0.0);
        let mut seeds: Vec<(String, f64)> = ranked
            .iter()
            .take(SEEDS)
            .filter(|r| r.lexical >= SEED_WORDS || !r.tagged.is_empty())
            .map(|r| {
                (
                    r.id.clone(),
                    if top > 0.0 {
                        (r.score / top).min(1.0)
                    } else {
                        0.0
                    },
                )
            })
            .collect();
        seeds.extend(meaning_seeds(&ranked, &seeds));
        let mut skip: BTreeSet<String> = in_core.clone();
        for c in target.iter().chain(&recipes).chain(&facts).chain(&full) {
            skip.insert(c.id.clone());
            skip.extend(c.also.iter().cloned());
        }
        let terms: BTreeSet<String> = search::query_terms(&query).into_iter().collect();
        for near in crate::nearby::related(store, &seeds, &skip, &query, &terms, RELATED_CAP) {
            let Some(rec) = store.get(&near.id) else {
                continue;
            };
            related.push(chunk(
                near.id.clone(),
                // The title is cut, never the match it came from.
                format!(
                    "{} (← {})",
                    cut_line(
                        &format!("{}  {}", near.id, one(&slug_words(store, rec.title()))),
                        LINE_CHARS
                    ),
                    near.via
                ),
            ));
        }
    }
    // A task whose words match nothing well is told so, with the nearest conventions, instead
    // of a bundle padded with rules that share one word with it.
    if weak {
        let mut text = String::from(if ranked.is_empty() {
            "No convention holds any of this task's words.\n"
        } else {
            "No convention holds two of this task's words, so nothing below is a match. The nearest, by the words they share:\n"
        });
        for r in ranked.iter().take(DID_YOU_MEAN) {
            if let Some(rec) = store.get(&r.id) {
                let line = crate::relate::group(store, rec).line;
                text.push_str(&format!("{}\n", cut_line(&line, LINE_CHARS)));
            }
        }
        text.push_str(
            "Ask again with the words the rule would use (a file, a screen, the failure), or `trailmark find <word>`.\n",
        );
        full.push(chunk("did-you-mean", text));
        lines.clear();
    }

    // Shares of the pool after the core. The tail ("Not included") is reserved.
    let reserve = 220usize;
    let pool = req.budget.saturating_sub(core_tok).saturating_sub(reserve);
    // Target, open items, recipes ("How to run"), facts, rules in full, related,
    // rules in a line. The full text is capped by count (`FULL_CAP`); what it leaves goes on.
    let weights = WEIGHTS;
    let mut share = [0usize; 7];
    let mut left = pool;
    for (i, w) in weights.iter().enumerate() {
        let room = if i + 1 == weights.len() {
            left
        } else {
            pool * w / 100
        };
        share[i] = room.min(left);
        left = left.saturating_sub(share[i]);
    }

    let sections = [
        &mut target,
        &mut opens,
        &mut recipes,
        &mut facts,
        &mut full,
        &mut related,
        &mut lines,
    ];
    // Fill in order, carrying unused share forward.
    let mut carry = 0usize;
    let mut kept: Vec<Vec<Chunk>> = Vec::new();
    for (i, chunks) in sections.iter().enumerate() {
        let room = share[i] + carry;
        let (keep, used) = if i == 5 || i == 6 {
            // Related and the one-line list continue after what is shown: nothing twice.
            let mut shown = BTreeSet::new();
            for k in &kept {
                shown.extend(shown_ids(k));
            }
            let rest: Vec<Chunk> = chunks
                .iter()
                .filter(|c| !shown.contains(&c.id))
                .map(Chunk::copy)
                .collect();
            take_until(&rest, room)
        } else if i == 0 {
            // The target is what the bundle is for: a review or loop record longer than its
            // share is cut, each to an equal part of the room, never dropped whole. Dropping
            // it whole left a review bundle with neither the review, its loop, nor the step.
            let cap = (room / chunks.len().max(1)).max(150);
            let cut: Vec<Chunk> = chunks.iter().map(|c| cut_chunk(c, cap)).collect();
            take_until(&cut, room)
        } else {
            take_until(chunks, room)
        };
        carry = room.saturating_sub(used);
        kept.push(keep);
    }

    let tail = |kept: &[Vec<Chunk>]| not_included(store, kept, facts_all);
    let mut body = assemble(&label, &core, &kept, &tail(&kept));
    // Trim one-line, then related, then full rules, then facts, then recipes, then opens, then
    // target, until the bundle fits.
    let order = [6usize, 5, 4, 3, 2, 1, 0];
    loop {
        let rendered = finish_bundle(&label, &body, req.budget);
        if tok4(&rendered) <= req.budget {
            body = rendered;
            break;
        }
        let mut trimmed = false;
        for idx in order {
            if kept[idx].pop().is_some() {
                body = assemble(&label, &core, &kept, &tail(&kept));
                trimmed = true;
                break;
            }
        }
        if !trimmed {
            return Err(Fail::Over(Over {
                rel: "index/core.md".into(),
                tok: core_tok,
                budget: req.budget,
            }));
        }
    }

    let mut included = vec!["core".into()];
    let mut explain = String::new();
    for (chunks, heading) in kept.iter().zip([
        "target",
        "open",
        "recipe",
        "fact",
        "rule",
        "related",
        "rule-line",
    ]) {
        for c in chunks {
            included.push(format!("{heading} {}", c.id));
            if let Some(r) = ranked.iter().find(|r| r.id == c.id) {
                explain.push_str(&format!("{heading} {}  {}\n", c.id, r.why()));
            }
        }
    }
    let omitted = tail(&kept)
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    if req.explain {
        // After the measured bundle, so the budget line stays true; a person debugging the
        // ranking reads it, an agent does not need it.
        body.push_str("\n<!-- trailmark:explain — not counted in tok4 -->\n## Why these rules\n");
        if req.areas.is_empty()
            && req.loop_id.is_none()
            && req.review_id.is_none()
            && req.item_id.is_none()
        {
            for e in area_evidence(store, &query) {
                body.push_str(&format!(
                    "area {}: {} of the task's words name it; {} of the {} best matches of its other words are in it; {}\n",
                    e.name,
                    e.words,
                    e.held,
                    e.sample,
                    if e.kept { "kept" } else { "dropped" }
                ));
            }
        }
        body.push_str(&explain);
    }
    Ok(Bundle {
        tok: tok4(&body),
        budget: req.budget,
        markdown: body,
        included,
        omitted,
    })
}

fn meaning_seeds(_: &[Ranked], _: &[(String, f64)]) -> Vec<(String, f64)> {
    Vec::new()
}

/// The Now line of the committed core knows this branch only. A bundle is read now, so it
/// also names what runs in the other worktrees, on its own line after the Now line.
fn with_elsewhere(core: String, elsewhere: &[(String, String)]) -> String {
    if elsewhere.is_empty() {
        return core;
    }
    let line = format!(
        "Running in other worktrees: {}.",
        elsewhere
            .iter()
            .map(|(id, wt)| format!("{id} ({wt})"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let Some(at) = core.find("## Now\n") else {
        return core;
    };
    let after = at + "## Now\n".len();
    let end = core[after..]
        .find('\n')
        .map(|i| after + i + 1)
        .unwrap_or(core.len());
    format!("{}{line}\n{}", &core[..end], &core[end..])
}

impl Chunk {
    fn copy(&self) -> Chunk {
        Chunk {
            id: self.id.clone(),
            text: self.text.clone(),
            also: self.also.clone(),
        }
    }
}

fn shown_ids(chunks: &[Chunk]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in chunks {
        out.insert(c.id.clone());
        out.extend(c.also.iter().cloned());
    }
    out
}

/// The "Not included" tail (g): what was left out and the command that fetches it.
fn not_included(store: &Store, kept: &[Vec<Chunk>], facts_all: usize) -> String {
    let mut shown = BTreeSet::new();
    for chunks in kept {
        shown.extend(shown_ids(chunks));
    }
    let count = |want: &dyn Fn(&Record) -> bool| {
        store
            .records
            .iter()
            .filter(|r| want(r) && !shown.contains(&r.id_str()))
            .count()
    };
    let rules = count(&|r| r.type_name() == "rule" && !is_tombstone(r.status()));
    let opens = count(&|r| r.type_name() == "open" && r.status() == "open");
    let decisions = count(&|r| r.type_name() == "decision" && r.status() == "active");
    let facts = facts_all.saturating_sub(
        kept.get(2).map(|c| c.len()).unwrap_or(0) + kept.get(3).map(|c| c.len()).unwrap_or(0),
    );
    let mut parts = Vec::new();
    if rules > 0 {
        parts.push(format!(
            "{rules} other active rules (trailmark find <words> --type rule)"
        ));
    }
    if opens > 0 {
        parts.push(format!(
            "{opens} other open items (trailmark find <words> --type open)"
        ));
    }
    if decisions > 0 {
        parts.push(format!(
            "{decisions} other decisions (trailmark find <words> --type decision)"
        ));
    }
    if facts > 0 {
        parts.push(format!(
            "{facts} other facts (trailmark find <words> --type fact)"
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("## Not included\n\n{}\n", parts.join(" · "))
}

fn resolve_target(
    store: &Store,
    req: &Request,
) -> Result<(String, String, Vec<String>, Vec<String>), Fail> {
    if let Some(id) = &req.loop_id {
        let rec = must(store, id)?;
        let mut brief = part_text(rec, "brief");
        // A loop added without `--part brief=` says `Pending.` until its close, so its bundle
        // was ranked on the title alone and surfaced no rule for the work. Its
        // item says what the work is: rank on that too, and the target says how to ask on
        // the task's own words (`target_block`).
        if pending(&brief) {
            if let Some(item) = rec.fm.get_str("item").and_then(|i| store.get(i)) {
                brief = format!("{}\n{}", item.title(), item.body);
            }
        }
        let query = format!("{} {}\n{brief}", rec.title(), req.task);
        let areas = areas_of(store, rec, &query, &req.areas);
        let platforms = rec.fm.strings("platform");
        return Ok((rec.id_str(), query, areas, platforms));
    }
    if let Some(id) = &req.review_id {
        let rec = must(store, id)?;
        let loop_id = rec
            .fm
            .strings("reviews")
            .into_iter()
            .next()
            .unwrap_or_default();
        let brief = store
            .get(&loop_id)
            .map(|r| part_text(r, "brief"))
            .unwrap_or_default();
        let query = format!("{} {}\n{brief}", rec.title(), req.task);
        let areas = areas_of(store, rec, &query, &req.areas);
        let platforms = store
            .get(&loop_id)
            .map(|r| r.fm.strings("platform"))
            .unwrap_or_default();
        return Ok((rec.id_str(), query, areas, platforms));
    }
    if let Some(id) = &req.item_id {
        let rec = must(store, id)?;
        let query = format!("{} {}\n{}", rec.title(), req.task, rec.body);
        let areas = areas_of(store, rec, &query, &req.areas);
        let platforms = rec.fm.strings("platform");
        return Ok((rec.id_str(), query, areas, platforms));
    }
    if req.task.trim().is_empty() {
        return Err(Fail::Usage(
            "context needs a task, --loop, --review, --item, or --core".into(),
        ));
    }
    let areas = if req.areas.is_empty() {
        infer_areas(store, &req.task)
    } else {
        req.areas.clone()
    };
    Ok(("task".into(), req.task.clone(), areas, Vec::new()))
}

fn must<'a>(store: &'a Store, id: &str) -> Result<&'a Record, Fail> {
    let canon = Id::parse(id)
        .map(|i| i.canonical())
        .unwrap_or_else(|| id.to_string());
    store.get(&canon).ok_or_else(|| {
        // A review's bundle is asked for before its record exists;
        // say the command, not only the miss.
        let hint = match canon.strip_prefix("REV-") {
            Some(key) => format!(" — start it with `trailmark add review --for {key}`"),
            None => String::new(),
        };
        Fail::Missing(format!("unknown id {canon}{hint}"))
    })
}

fn areas_of(store: &Store, rec: &Record, query: &str, explicit: &[String]) -> Vec<String> {
    if !explicit.is_empty() {
        return explicit.to_vec();
    }
    let own = rec.areas();
    if !own.is_empty() {
        return own;
    }
    infer_areas(store, query)
}

/// The areas a task is about. A keyword names a candidate (as whole words: found "auth"
/// matching "author"); the candidate is kept only when the rules the task's own words find
/// first live in it. One domain word in passing ("…, deploy now line" in a task about trailmark)
/// named the deploy area, and the area's bonus then put every deploy rule that shared any
/// word with the task above the task's topic.
fn infer_areas(store: &Store, text: &str) -> Vec<String> {
    area_evidence(store, text)
        .into_iter()
        .filter(|e| e.kept)
        .map(|e| e.name)
        .collect()
}

/// One keyword-named area and what confirmed or refused it; `--explain` prints these.
pub struct AreaEvidence {
    pub name: String,
    /// How many of the task's words are the area's keywords.
    pub words: usize,
    pub held: usize,
    pub sample: usize,
    pub kept: bool,
}

fn area_evidence(store: &Store, text: &str) -> Vec<AreaEvidence> {
    let words = search::terms_light(text);
    let candidates: Vec<(&String, &crate::config::Area)> = store
        .config
        .areas
        .iter()
        .filter(|(_, area)| area.keywords.iter().any(|k| search::has_phrase(&words, k)))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let (rules, docs) = rule_docs(store);
    let q = search::query_terms(text);
    let mut out: Vec<AreaEvidence> = candidates
        .into_iter()
        .map(|(name, area)| {
            // The rest of the task, without the words that named the area: does it lead
            // into the area on its own?
            let named: BTreeSet<String> = area
                .keywords
                .iter()
                .flat_map(|k| search::terms(k))
                .collect();
            let rest: Vec<String> = q.iter().filter(|t| !named.contains(*t)).cloned().collect();
            let top: Vec<&Record> = bm25(&docs, &rest)
                .into_iter()
                .filter(|(_, s)| *s > 0.0)
                .take(AREA_SAMPLE)
                .filter_map(|(id, _)| rules.iter().find(|r| r.id_str() == id).copied())
                .collect();
            let held = top.iter().filter(|r| r.areas().contains(name)).count();
            let words = q.len() - rest.len();
            // Kept when the rest of the task leads into the area, or when the area's own words
            // are a fair share of the task ("CSRF, auth, session cookie, XSS" is a security
            // task even where the rest of its words lead elsewhere). A task that is nothing
            // but the area's words has nothing else to ask.
            let kept = top.is_empty()
                || held as f64 / top.len() as f64 >= AREA_SHARE
                || words as f64 / q.len().max(1) as f64 >= AREA_WORDS;
            AreaEvidence {
                name: name.clone(),
                words,
                held,
                sample: top.len(),
                kept,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// How many of the task's best word matches are sampled to confirm an area, and the share of
/// them that must belong to it.
const AREA_SAMPLE: usize = 20;
const AREA_SHARE: f64 = 0.25;
/// The share of the task's words that, when they are the area's own keywords, keep the area.
const AREA_WORDS: f64 = 0.2;

/// How much of a rule's word score comes from its best clause of the task, against the task
/// as a whole.
const CLAUSE_WEIGHT: f64 = 0.3;

/// A word held by more than this share of the rules carries no topic in this store ("first",
/// "page", "after", "line"): it is dropped from a task that has other words.
const STOP_SHARE: f64 = 0.1;

/// Below this many rules a share says nothing about a word: in a store of four rules every
/// word is held by a quarter of them (a cold read: a new store's bundle lost every rule).
const STOP_MIN_RULES: f64 = 100.0;

/// The task's words without the ones nearly every rule holds — as long as at least three
/// words the store holds remain; a task made only of ordinary words keeps them all, and a
/// small store keeps every word.
fn topic_terms(q: Vec<String>, df: &std::collections::HashMap<&str, usize>, n: f64) -> Vec<String> {
    if n < STOP_MIN_RULES {
        return q;
    }
    let held = |t: &String| df.get(t.as_str()).copied().unwrap_or(0);
    let kept: Vec<String> = q
        .iter()
        .filter(|t| held(t) as f64 / n <= STOP_SHARE)
        .cloned()
        .collect();
    if kept.iter().filter(|t| held(t) > 0).count() >= 3 {
        kept
    } else {
        q
    }
}

/// The rules a bundle may carry, with the words each is ranked on: title three times, as in
/// `find`, then the short form and the rule's own part.
fn rule_docs(store: &Store) -> (Vec<&Record>, Vec<(String, Vec<String>)>) {
    let rules: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "rule" && !is_tombstone(r.status()))
        .filter(|r| !never_whole(store, &r.rel))
        .collect();
    let docs = rules
        .iter()
        .map(|r| {
            let mut toks = Vec::new();
            for _ in 0..3 {
                toks.extend(search::terms(r.title()));
            }
            toks.extend(search::terms(r.fm.get_str("short").unwrap_or("")));
            if let Some(part) = rule_part_text(r) {
                toks.extend(search::terms(&part));
            }
            // The files a rule names in `links`
            // are words of the rule: a task that names the file finds the rule.
            for link in r.fm.strings("links") {
                if link.contains('/') {
                    toks.extend(search::terms(&link));
                }
            }
            (r.id_str(), toks)
        })
        .collect();
    (rules, docs)
}

/// The records a bundle ranks for the task: the rules, and the decisions and the facts that
/// are not recipes. A decision is a convention the task may need as much as a rule, and it
/// had no way into a bundle but its loop's link. A fact is chosen by its tags. A decision is
/// ranked on its title and its decision part, a fact on its title and its "How to apply"
/// (its body can be a log that names every subject).
fn ranked_docs(store: &Store) -> (Vec<&Record>, Vec<(String, Vec<String>)>) {
    let (mut recs, mut docs) = rule_docs(store);
    for r in store.records.iter() {
        let text = match r.type_name() {
            // A pinned decision is a principle the core prints in every bundle.
            "decision"
                if !is_tombstone(r.status()) && r.status() != "superseded" && !r.pinned() =>
            {
                record::find_part(&record::parts_of(r), "decision")
                    .map(|p| p.text.clone())
                    .unwrap_or_else(|| r.body.chars().take(1200).collect())
            }
            "fact" if r.status() == "active" && !is_recipe(r) => how_line(&r.body),
            _ => continue,
        };
        if never_whole(store, &r.rel) {
            continue;
        }
        let mut toks = Vec::new();
        for _ in 0..3 {
            toks.extend(search::terms(&slug_words(store, r.title())));
        }
        toks.extend(search::terms(&text));
        recs.push(r);
        docs.push((r.id_str(), toks));
    }
    (recs, docs)
}

/// A record whose title is a file slug (`<project>-rye-before-salt`) is ranked on its
/// words, without the project's name. Every such title carries that name and no rule says it.
/// As a word it was rare among the rules, so a task that names the project listed twenty
/// records by that word alone. `rank_rules` also drops the name from the task.
pub(crate) fn slug_words(store: &Store, title: &str) -> String {
    if title.contains(' ') {
        return title.to_string();
    }
    let words = title.replace(['-', '_'], " ");
    let project = store.config.name.as_deref().unwrap_or("").to_lowercase();
    match words.split_once(' ') {
        Some((first, rest)) if !project.is_empty() && first.to_lowercase() == project => {
            rest.to_string()
        }
        _ => words,
    }
}

/// A record, or the live record at the end of its `superseded_by` chain.
fn current<'a>(store: &'a Store, id: &str) -> Option<&'a Record> {
    let mut rec = store.get(id)?;
    for _ in 0..16 {
        if rec.status() != "superseded" {
            break;
        }
        match rec
            .fm
            .strings("superseded_by")
            .first()
            .and_then(|next| store.get(next))
        {
            Some(next) => rec = next,
            None => break,
        }
    }
    Some(rec)
}

fn target_chunks(store: &Store, req: &Request, label: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    if let Some(id) = req.loop_id.as_ref().or(req.item_id.as_ref()) {
        if let Some(rec) = store.get(id) {
            out.push(chunk(
                rec.id_str(),
                target_block(store, rec, !req.task.trim().is_empty()),
            ));
            push_linked(store, rec, &mut out);
            push_follows(store, rec, &mut out);
        }
    }
    if let Some(id) = &req.review_id {
        if let Some(rec) = store.get(id) {
            out.push(chunk(
                rec.id_str(),
                target_block(store, rec, !req.task.trim().is_empty()),
            ));
            push_follows(store, rec, &mut out);
            for loop_id in rec.fm.strings("reviews") {
                if let Some(loop_rec) = store.get(&loop_id) {
                    let mut text = format!("## {}\n\n", loop_rec.id_str());
                    for name in ["brief", "built", "not-verified"] {
                        if let Some(p) = record::find_part(&record::parts_of(loop_rec), name) {
                            text.push_str(&p.text);
                            text.push('\n');
                        }
                    }
                    out.push(chunk(loop_rec.id_str(), text));
                    push_follows(store, loop_rec, &mut out);
                }
            }
            // The review step named by `review_step` in trail.toml, or the record that
            // superseded it. A retired step's text names files that no longer exist.
            if let Some(id) = store.config.review_step.clone() {
                if let Some(step) = current(store, &id) {
                    out.push(chunk(
                        step.id_str(),
                        format!("{}  {}\n\n{}", step.id_str(), step.title(), step.body),
                    ));
                }
            }
            let _ = label;
        }
    }
    if req.loop_id.is_none() && req.review_id.is_none() && req.item_id.is_none() {
        out.push(chunk("task", format!("Task: {}\n", req.task.trim())));
    }
    out
}

/// At most this many earlier loops in a bundle, one line each.
const EARLIER_CAP: usize = 3;

/// Earlier loops a task like this one needed: a loop a past alike loop recorded as `missed`,
/// or one whose `applies_to` tag the task says. As with a missed rule, likeness alone is not
/// enough: the task
/// must also name the loop, by a tag or by two of the words of its title. One line each; the
/// agent asks for the part it needs.
fn earlier_work(store: &Store, query: &str, own: &[String]) -> Vec<Chunk> {
    let (_, docs) = rule_docs(store);
    let df = search::doc_freq(&docs);
    let n = docs.len().max(1) as f64;
    let q = topic_terms(search::query_terms(query), &df, n);
    let toks = tag_tokens(query);
    let missed = missed_for(store, &q, &df, n, own);
    let mut picked: Vec<(u8, f64, &Record, String)> = Vec::new();
    for rec in &store.records {
        if rec.type_name() != "loop" || own.contains(&rec.id_str()) {
            continue;
        }
        let said = rec
            .fm
            .strings("applies_to")
            .into_iter()
            .any(|t| tag_said(&t, &toks));
        let title: BTreeSet<String> = search::terms(&slug_words(store, rec.title()))
            .into_iter()
            .collect();
        let titled = q.iter().filter(|t| title.contains(*t)).count();
        let why = match missed.get(&rec.id_str()) {
            Some((by, alike)) if said || titled >= 2 => (0u8, *alike, format!("missed by {by}")),
            _ if said => (1u8, 0.0, "tagged for this".to_string()),
            _ => continue,
        };
        picked.push((why.0, why.1, rec, why.2));
    }
    picked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.2.id_str().cmp(&b.2.id_str()))
    });
    picked
        .into_iter()
        .take(EARLIER_CAP)
        .map(|(_, _, rec, why)| {
            // A migrated loop is one section with no parts: show it whole.
            let part = if record::find_part(&record::parts_of(rec), "built").is_some() {
                " --part built"
            } else {
                ""
            };
            chunk(
                rec.id_str(),
                format!(
                    "Earlier work: {}  {} ({why}; `trailmark show {}{part}`)\n",
                    rec.id_str(),
                    one(rec.title()),
                    rec.id_str()
                ),
            )
        })
        .collect()
}

fn target_block(store: &Store, rec: &Record, task_given: bool) -> String {
    let mut text = format!("{} ({})", rec.id_str(), rec.status());
    if let Some(item) = rec.fm.get_str("item") {
        text.push_str(&format!(" · item {item}"));
    }
    if let Some(note) = reservation_note(store, &rec.id_str()) {
        text.push_str(&format!(" · {note}"));
    }
    text.push_str("\n\n");
    if rec.type_name() == "loop" {
        if let Some(p) = record::find_part(&record::parts_of(rec), "brief") {
            // The part carries its own `## Brief` heading (a cold reader saw it twice).
            text.push_str(&p.text);
            if pending(&p.text) && rec.status() == "running" && !task_given {
                let key = rec.id_str().trim_start_matches("LOOP-").to_string();
                text.truncate(text.trim_end().len());
                text.push_str(&format!(
                    "\n\nThe brief is not written yet, so the rules below were ranked on the title{}. Ask on the task's own words: `trailmark context --loop {key} \"<the brief in your words>\"` (or add the loop with `--part brief=<file>`).\n",
                    if rec.fm.get_str("item").map(|i| store.get(i).is_some()).unwrap_or(false) { " and its item" } else { " alone" }
                ));
            }
        }
    } else {
        text.push_str(&rec.body);
    }
    text
}

fn reservation_note(store: &Store, for_id: &str) -> Option<String> {
    let mut parts = Vec::new();
    for kind in ["rule", "open"] {
        if let Some(block) = store
            .reservations
            .iter()
            .find(|r| r.type_name == kind && r.for_id == for_id)
        {
            let scheme = crate::id::scheme();
            let label = if kind == "rule" {
                if scheme.section_rules() {
                    format!("{}-2.{}–{}", scheme.rule, block.from, block.to)
                } else {
                    format!("{}-{}–{}", scheme.rule, block.from, block.to)
                }
            } else {
                format!("{}-{:04}–{:04}", scheme.open, block.from, block.to)
            };
            parts.push(label);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("reserved {}", parts.join(", ")))
    }
}

fn push_follows(store: &Store, rec: &Record, out: &mut Vec<Chunk>) {
    if let Some(block) = crate::relate::followed_block(store, rec) {
        out.push(Chunk {
            id: block.id,
            text: block.text,
            also: block.also,
        });
    }
}

fn push_linked(store: &Store, rec: &Record, out: &mut Vec<Chunk>) {
    let mut ids = rec.fm.strings("decisions");
    if let Some(item) = rec.fm.get_str("item").and_then(|id| store.get(id)) {
        out.push(chunk(
            item.id_str(),
            format!("{}  {}\n\n{}", item.id_str(), item.title(), item.body),
        ));
        ids.extend(item.fm.strings("decisions"));
    }
    ids.extend(rec.fm.strings("links"));
    ids.sort();
    ids.dedup();
    for id in ids {
        let Some(other) = store.get(&id) else {
            continue;
        };
        if other.type_name() != "decision" || is_tombstone(other.status()) {
            continue;
        }
        let part = record::find_part(&record::parts_of(other), "decision")
            .map(|p| p.text.clone())
            .unwrap_or_default();
        out.push(chunk(
            other.id_str(),
            format!("{}  {}\n\n{part}", other.id_str(), other.title()),
        ));
    }
}

fn open_chunks(store: &Store, areas: &[String]) -> Vec<Chunk> {
    let mut items: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "open" && r.status() == "open")
        .filter(|r| {
            if areas.is_empty() {
                r.fm.get_str("owner") == Some("author")
                    || matches!(r.fm.get_str("severity"), Some("high" | "medium"))
            } else {
                r.areas().iter().any(|a| areas.iter().any(|b| a == b))
            }
        })
        .collect();
    items.sort_by(|a, b| {
        open_rank(a)
            .cmp(&open_rank(b))
            .then(a.id_str().cmp(&b.id_str()))
    });
    items
        .into_iter()
        .map(|r| {
            chunk(
                r.id_str(),
                format!(
                    "{}  {}  {}  {}",
                    r.id_str(),
                    r.fm.get_str("severity").unwrap_or(""),
                    r.fm.get_str("owner").unwrap_or(""),
                    one(r.title())
                ),
            )
        })
        .collect()
}

fn open_rank(rec: &Record) -> (u8, u8) {
    let owner = match rec.fm.get_str("owner") {
        Some("author") => 0,
        Some("orchestrator") => 1,
        Some("next-loop") => 2,
        _ => 3,
    };
    let sev = match rec.fm.get_str("severity") {
        Some("high") => 0,
        Some("medium") => 1,
        Some("low") => 2,
        _ => 3,
    };
    (owner, sev)
}

/// Facts that bear on this task: the ones whose `applies_to` names the task's areas or
/// platforms, and the ones whose words match the task. A fact with no `applies_to` is not
/// in every bundle; it has to match the task's words. At most `FACT_CAP`, best first.
fn fact_chunks(
    store: &Store,
    query: &str,
    areas: &[String],
    platforms: &[String],
    pointed: &std::collections::BTreeMap<String, Vec<String>>,
) -> (Vec<Chunk>, usize) {
    const FACT_CAP: usize = 5;
    let facts: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| r.type_name() == "fact" && r.status() == "active" && !is_recipe(r))
        .collect();
    // A fact is judged by its title and its "How to apply", not by its whole body: some
    // facts have grown into logs that mention every subject.
    let face = |r: &Record| format!("{}\n{}", r.title(), how_line(&r.body));
    let docs: Vec<(String, Vec<String>)> = facts
        .iter()
        .map(|r| (r.id_str(), search::terms(&face(r))))
        .collect();
    let q = search::query_terms(query);
    let toks = tag_tokens(query);
    let bm: std::collections::BTreeMap<String, f64> = bm25(&docs, &q).into_iter().collect();
    let mut scored: Vec<(f64, &Record)> = Vec::new();
    for r in facts {
        let applies = r.fm.strings("applies_to");
        let named = applies
            .iter()
            .any(|a| areas.iter().any(|b| a == b) || platforms.iter().any(|b| a == b));
        // A tag the task's words say ("probe") brings a fact, as it brings a rule or a
        // recipe. A fact used to come only by an area or a platform the task was in, so a
        // fact tagged for its situation never reached a task that named that situation.
        let said = applies.iter().filter(|a| tag_said(a, &toks)).count();
        let lexical = bm.get(&r.id_str()).copied().unwrap_or(0.0);
        let words: BTreeSet<String> = search::terms(&face(r)).into_iter().collect();
        let shared = q.iter().filter(|t| words.contains(*t)).count();
        // A fact with no `applies_to` must share two of the task's words: one shared word
        // ("note") is how every log-like fact got into every bundle.
        let by_term = pointed.get(&r.id_str()).map(|n| n.len()).unwrap_or(0);
        let keep = named || said > 0 || by_term > 0 || (applies.is_empty() && shared >= 2);
        if keep {
            // A term the agent named ranks its facts above a tag or an area.
            let bonus = if by_term > 0 {
                200.0 + by_term as f64
            } else if said > 0 {
                100.0 + said as f64
            } else if named {
                100.0
            } else {
                0.0
            };
            scored.push((lexical + bonus, r));
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.id_str().cmp(&b.1.id_str()))
    });
    let total = store
        .records
        .iter()
        .filter(|r| r.type_name() == "fact" && r.status() == "active")
        .count();
    let chunks = scored
        .into_iter()
        .take(FACT_CAP)
        .map(|(_, r)| {
            let how = how_line(&r.body);
            chunk(
                r.id_str(),
                format!("{}  {}\n{how}", r.id_str(), one(r.title())),
            )
        })
        .collect();
    (chunks, total)
}

fn is_recipe(rec: &Record) -> bool {
    rec.type_name() == "fact" && rec.fm.get_str("kind") == Some("recipe")
}

/// The tags of a record's `applies_to` this task names: an area or a platform of the task, or
/// a tag the task says as words ("worktree", "release stack"). An area named by a tag counts
/// only when `with_areas`: for a rule the area already has its own bonus.
fn tags_named(
    rec: &Record,
    toks: &[String],
    areas: &[String],
    platforms: &[String],
    with_areas: bool,
) -> Vec<String> {
    rec.fm
        .strings("applies_to")
        .into_iter()
        .filter(|tag| {
            (with_areas && areas.iter().any(|a| a == tag))
                || platforms.iter().any(|p| p == tag)
                || tag_said(tag, toks)
        })
        .collect()
}

/// The task's words as a tag is matched against them: lowercase, unstemmed, stop words out.
fn tag_tokens(query: &str) -> Vec<String> {
    search::tokenize(query)
        .into_iter()
        .filter(|t| !search::is_stop(t))
        .collect()
}

/// True when the task says the tag: its words side by side, in order, each the word itself or
/// its plural. Not by stem, and not scattered: "boarding" is not "board", "stacked
/// cards" is not a stack, and a task that says "before" in one clause and "after" in another
/// does not name `before-after`.
fn tag_said(tag: &str, toks: &[String]) -> bool {
    let want = search::tokenize(tag);
    if want.is_empty() || want.len() > toks.len() {
        return false;
    }
    let same = |t: &String, w: &String| {
        t == w
            || t.strip_suffix('s').is_some_and(|b| b == w)
            || t.strip_suffix("es").is_some_and(|b| b == w)
    };
    toks.windows(want.len())
        .any(|win| win.iter().zip(&want).all(|(t, w)| same(t, w)))
}

/// The values of a loop's `platform` (C02).
const PLATFORMS: &[&str] = &["web", "server", "mobile", "infra", "docs"];

/// The situations every loop and review is in: a worktree of its own, landed by a commit on
/// main.
const WORKTREE_TAGS: &[&str] = &["worktree", "commit"];

/// At most this many recipes in a bundle ("How to run"): two to four commands a loop can
/// copy, not a manual.
const RECIPE_CAP: usize = 4;

/// Recipes (facts of kind `recipe`) for this task: the ones with an `applies_to` tag the
/// task's words name, ranked by those tags (an area or a platform of the task counts half),
/// then by their words. A recipe without `applies_to` has to share two of the task's words. Each is printed as its first part —
/// when, and the commands — with its id for the rest.
fn recipe_chunks(
    store: &Store,
    query: &str,
    areas: &[String],
    platforms: &[String],
    in_worktree: bool,
    pointed: &std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<Chunk> {
    let recipes: Vec<&Record> = store
        .records
        .iter()
        .filter(|r| is_recipe(r) && r.status() == "active")
        .collect();
    let toks = tag_tokens(query);
    let docs: Vec<(String, Vec<String>)> = recipes
        .iter()
        .map(|r| {
            (
                r.id_str(),
                search::terms(&format!("{}\n{}", r.title(), r.body)),
            )
        })
        .collect();
    let q = search::query_terms(query);
    let bm: std::collections::BTreeMap<String, f64> = bm25(&docs, &q).into_iter().collect();
    let top = bm.values().cloned().fold(0.0f64, f64::max);
    let mut scored: Vec<(f64, &Record)> = Vec::new();
    for r in recipes {
        // A tag the task's own words name ("worktree", "submodule") says more than an area or a
        // platform the task is in: nearly every recipe is for `web`.
        // An area's or a platform's name is not a situation: "the web" in a deploy task is
        // the deploy's target, not a reason to set up the app's worktree.
        let by_words = tags_named(r, &toks, &[], &[], false)
            .iter()
            .filter(|t| !store.config.areas.contains_key(*t) && !PLATFORMS.contains(&t.as_str()))
            .count();
        let all = tags_named(r, &toks, areas, platforms, true).len();
        // A loop's or a review's own bundle names `worktree` and `commit` for it, below any
        // tag the task's words name.
        let implied = if in_worktree {
            r.fm.strings("applies_to")
                .iter()
                .filter(|t| WORKTREE_TAGS.contains(&t.as_str()) && !tag_said(t, &toks))
                .count()
        } else {
            0
        };
        let named =
            by_words as f64 + 0.5 * all.saturating_sub(by_words) as f64 + 0.5 * implied as f64;
        let lexical = if top > 0.0 {
            bm.get(&r.id_str()).copied().unwrap_or(0.0) / top
        } else {
            0.0
        };
        let shared = docs
            .iter()
            .find(|(id, _)| id == &r.id_str())
            .map(|(_, t)| q.iter().filter(|w| t.contains(w)).count())
            .unwrap_or(0);
        // At least one tag the task's words name: an area alone put the app's worktree
        // recipes into a deploy's bundle, which runs from a clean checkout.
        // A term the agent named leads to its recipes first.
        let by_term = pointed.get(&r.id_str()).map(|n| n.len()).unwrap_or(0);
        let keep = by_words > 0
            || implied > 0
            || by_term > 0
            || (r.fm.strings("applies_to").is_empty() && shared >= 2);
        if keep {
            let term = TERM_BONUS * by_term.min(TERM_MAX) as f64;
            scored.push((named + lexical + term, r));
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.id_str().cmp(&b.1.id_str()))
    });
    scored
        .into_iter()
        .take(RECIPE_CAP)
        .map(|(_, r)| chunk(r.id_str(), recipe_text(r)))
        .collect()
}

/// A recipe as a bundle prints it: the id and title, then the body up to `**Why:**`.
fn recipe_text(rec: &Record) -> String {
    let head = rec.body.split("**Why:**").next().unwrap_or("").trim_end();
    format!("{}  {}\n{head}\n", rec.id_str(), one(rec.title()))
}

fn how_line(body: &str) -> String {
    let mut out = Vec::new();
    for line in body
        .lines()
        .skip_while(|l| !l.contains("**How to apply:**"))
    {
        if !out.is_empty() && line.trim().is_empty() {
            break;
        }
        out.push(line);
        if out.len() == 4 {
            break;
        }
    }
    out.join("\n")
}

pub struct Ranked {
    pub id: String,
    pub score: f64,
    pub lexical: f64,
    pub area: bool,
    pub cited: usize,
    pub titled: usize,
    pub paired: usize,
    pub anchor: bool,
    /// The rule's `applies_to` tags this task names.
    pub tagged: Vec<String>,
    /// The past loop that missed this rule for a task like this one, and how alike.
    pub missed: Option<(String, f64)>,
    /// How many distinct words of the task the record holds ("did you mean").
    pub shared: usize,
    /// What the task's tags added to its word score.
    pub expanded: f64,
    /// The `--terms` that point to it.
    pub termed: Vec<String>,
}

impl Ranked {
    fn why(&self) -> String {
        format!(
            "score {:.2}: words {:.2}, title words {}, word pairs {}, area {}, cited {}{}",
            self.score,
            self.lexical,
            self.titled,
            self.paired,
            if self.area { "yes" } else { "no" },
            self.cited,
            if self.anchor {
                ", anchor of its area"
            } else {
                ""
            }
        ) + &self.recall_note()
    }

    fn recall_note(&self) -> String {
        let mut out = String::new();
        if self.expanded > 0.0 {
            out.push_str(&format!(", by tags' words {:.2}", self.expanded));
        }
        if !self.tagged.is_empty() {
            out.push_str(&format!(", tags {}", self.tagged.join("+")));
        }
        if let Some((by, sim)) = &self.missed {
            out.push_str(&format!(", missed by {by} (alike {sim:.2})"));
        }
        if !self.termed.is_empty() {
            out.push_str(&format!(", terms {}", self.termed.join("+")));
        }
        out
    }
}

/// Per `applies_to` tag of a rule that the task names (its words or its platforms), up to
/// `TAG_MAX`. A tag is a situation the rule's words do not say, such as `worktree` or
/// `release-stack`.
const TAG_BONUS: f64 = 1.5;
const TAG_MAX: usize = 2;

/// A record a term of `--terms` points to gains `TERM_BONUS` per such term, up to `TERM_MAX`
/// terms: above a word match, so the records of the terms named lead the bundle; two
/// terms that agree on a record put it above either's other records. Tuned on the evaluation's
/// tune half only.
const TERM_BONUS: f64 = 4.0;
const TERM_MAX: usize = 2;

/// A rule a past loop recorded as missed (`missed:` on its loop or review record) gains up
/// to `MISS_BONUS` for a task like that loop's. "Like" is the share of the task's
/// topic words, weighted by how rare they are among the rules, that the loop's title and
/// brief hold: nothing below `MISS_FLOOR`, the whole bonus from `MISS_FULL`.
const MISS_BONUS: f64 = 3.0;
const MISS_FLOOR: f64 = 0.2;
const MISS_FULL: f64 = 0.45;
/// A missed rule gains its bonus only when the task names one of its tags or its words score
/// at least this share of the best rule's.
const MISS_HOOK: f64 = 0.3;
/// A loop is "like" the task only when they share at least this many topic words. One shared
/// word, often the project's name, was enough to call an unlike loop a match.
const MISS_MIN_SHARED: usize = 2;

/// The rules past loops missed, each with its best (loop, likeness) for this task. `own`
/// (the loop or review the bundle is for) is left out: a loop's own misses came after it.
fn missed_for(
    store: &Store,
    q: &[String],
    df: &std::collections::HashMap<&str, usize>,
    n: f64,
    own: &[String],
) -> std::collections::HashMap<String, (String, f64)> {
    let mut out: std::collections::HashMap<String, (String, f64)> =
        std::collections::HashMap::new();
    if q.is_empty() {
        return out;
    }
    let idf = |t: &str| (1.0 + n / (df.get(t).copied().unwrap_or(0) as f64 + 1.0)).ln();
    let total: f64 = q.iter().map(|t| idf(t)).sum();
    for rec in &store.records {
        if !matches!(rec.type_name(), "loop" | "review") || is_tombstone(rec.status()) {
            continue;
        }
        let missed = rec.fm.strings("missed");
        if missed.is_empty() || own.contains(&rec.id_str()) {
            continue;
        }
        let mut text = format!("{}\n{}", rec.title(), part_text(rec, "brief"));
        for loop_id in rec.fm.strings("reviews") {
            if own.contains(&loop_id) {
                continue;
            }
            if let Some(l) = store.get(&loop_id) {
                text.push_str(&format!("\n{}\n{}", l.title(), part_text(l, "brief")));
            }
        }
        let words: BTreeSet<String> = search::terms(&text).into_iter().collect();
        let shared: f64 = q
            .iter()
            .filter(|t| words.contains(*t))
            .map(|t| idf(t))
            .sum();
        let alike = if total > 0.0 { shared / total } else { 0.0 };
        let n_shared = q.iter().filter(|t| words.contains(*t)).count();
        if alike < MISS_FLOOR || n_shared < MISS_MIN_SHARED {
            continue;
        }
        for id in missed {
            let better = out.get(&id).map(|(_, a)| alike > *a).unwrap_or(true);
            if better {
                out.insert(id, (rec.id_str(), alike));
            }
        }
    }
    out
}

/// A word held by more than this share of the rules is an ordinary word of this store
/// ("page", "test", "state"): it still scores through BM25, but it does not count as a title
/// match of the task.
const COMMON_SHARE: f64 = 0.04;

/// Per distinct uncommon task word in a rule's title beyond the first, up to `TITLE_MAX`.
const TITLE_BONUS: f64 = 0.6;
const TITLE_MAX: usize = 3;

/// Per pair of adjacent task words found adjacent in a rule (`search::query_pairs`), up to
/// `PAIR_MAX`.
const PAIR_BONUS: f64 = 0.75;
const PAIR_MAX: usize = 2;

/// The weight of the words a task's tags lead to (`expansion`), against 1.0 for its own words.
const EXPAND_WEIGHT: f64 = 0.15;
/// At most this many such words.
const EXPAND_TERMS: usize = 12;

/// The words a task reaches through the store's tags. Each `trailmark tag <id> --add <word>`
/// says "a task that said <word> meant this record": it is an alias the store grew
/// from its misses. A task that says all the words of a tag is led to the uncommon words of
/// the titles that carry the tag, so a record that shares those words but not the task's own
/// gains a little. Area and platform names are not tags here, and nor is a word
/// most records hold.
fn expansion(
    store: &Store,
    q: &[String],
    df: &std::collections::HashMap<&str, usize>,
    n: f64,
) -> Vec<String> {
    let said: BTreeSet<&str> = q.iter().map(|t| t.as_str()).collect();
    let mut count: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for rec in &store.records {
        if !matches!(rec.type_name(), "rule" | "decision" | "fact") || is_tombstone(rec.status()) {
            continue;
        }
        let tags = rec.fm.strings("applies_to");
        let hit = tags.iter().any(|tag| {
            !store.config.areas.contains_key(tag) && !PLATFORMS.contains(&tag.as_str()) && {
                let words = search::terms(tag);
                !words.is_empty() && words.iter().all(|w| said.contains(w.as_str()))
            }
        });
        if !hit {
            continue;
        }
        let title: BTreeSet<String> = search::query_terms(&slug_words(store, rec.title()))
            .into_iter()
            .collect();
        for t in title {
            let d = df.get(t.as_str()).copied().unwrap_or(0);
            let uncommon = n < STOP_MIN_RULES || (d as f64 / n) <= COMMON_SHARE;
            if d > 0 && uncommon && !said.contains(t.as_str()) {
                *count.entry(t).or_insert(0) += 1;
            }
        }
    }
    let mut terms: Vec<(String, usize)> = count.into_iter().filter(|(_, c)| *c >= 2).collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    terms
        .into_iter()
        .take(EXPAND_TERMS)
        .map(|(t, _)| t)
        .collect()
}

/// A task word the store does not hold, of five letters or more, read as the store's word it
/// is a slip of: one letter dropped, one added or two neighbours swapped, the same first
/// letter, and a word at least three rules use ("reservaton" still asks for "reservation",
/// ). A substitution is not a slip: "faint" is not "paint", "stake" is not "state", and the
/// first version of this (any edit, two from eight letters) turned one in twelve words of the
/// repo's own vocabulary that the rules do not use into another word.
fn spelled(q: Vec<String>, df: &std::collections::HashMap<&str, usize>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in q {
        let fixed = if df.contains_key(t.as_str()) || t.chars().count() < 5 || !t.is_ascii() {
            t
        } else {
            df.iter()
                .filter(|(w, d)| **d >= SPELL_MIN_RULES && w.as_bytes()[0] == t.as_bytes()[0])
                .filter(|(w, _)| slip_of(&t, w))
                .map(|(w, d)| (std::cmp::Reverse(*d), w.to_string()))
                .min()
                .map(|(_, w)| w)
                .unwrap_or(t)
        };
        if !out.contains(&fixed) {
            out.push(fixed);
        }
    }
    out
}

/// A word is a spelling target only when this many rules use it.
const SPELL_MIN_RULES: usize = 3;

/// `typed` is `word` with one letter dropped, one added or two neighbours swapped (ASCII).
fn slip_of(typed: &str, word: &str) -> bool {
    let (a, b) = (typed.as_bytes(), word.as_bytes());
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        return diff.len() == 2
            && diff[1] == diff[0] + 1
            && a[diff[0]] == b[diff[1]]
            && a[diff[1]] == b[diff[0]];
    }
    let (long, short) = if a.len() > b.len() { (a, b) } else { (b, a) };
    if long.len() != short.len() + 1 {
        return false;
    }
    let at = (0..short.len())
        .find(|&i| long[i] != short[i])
        .unwrap_or(short.len());
    long[at + 1..] == short[at..]
}

/// Rules for a task, best first, with the changes measured on a real store.
///
/// 1. The word score is normalised to the best match of this query (0..1). Raw BM25 let one
///    rare word outweigh everything else, so a rule that shared one unusual word with the
///    task beat the rule the task was about.
/// 2. Words held by more than a tenth of the rules ("first", "page", "line") are dropped
///    from the task (`topic_terms`), so the task's rarer nouns carry the score. Squaring
///    the idf instead was measured and lost a task two on-point rules.
/// 3. Each clause of the task is also scored alone and normalised to its own best match;
///    `CLAUSE_WEIGHT` of the word score is the rule's best clause. A task with four subjects
///    gets rules for each, not only for the subject that has the most rules.
/// 4. A rule whose title holds several of the task's uncommon words gains `TITLE_BONUS`
///    for each after the first, and a rule holding a pair of the task's adjacent words
///    ("memory leak", "re-render") gains `PAIR_BONUS` per pair: the pair tells a homonym
///    ("a session cookie" against "a long session") apart.
/// 5. Area anchors: the most-cited rules of the task's areas that also share a word with the
///    task come first. A founding rule is cited everywhere and phrased generally; a lexical
///    score alone puts it behind narrow rules that happen to repeat the task's nouns. The
///    areas themselves are confirmed by the task's words (`infer_areas`), so a domain word
///    named in passing does not hand its whole area a bonus.
///
/// Ties break by id.
fn rank_rules(
    store: &Store,
    query: &str,
    areas: &[String],
    own: &[String],
    cites: &std::collections::BTreeMap<String, usize>,
    pointed: &std::collections::BTreeMap<String, Vec<String>>,
) -> (Vec<Ranked>, bool) {
    const ANCHORS: usize = 3;
    let (rules, docs) = ranked_docs(store);
    let df = search::doc_freq(&docs);
    let n = docs.len().max(1) as f64;
    // The project's name is on every page of its memories and in every brief: it is no topic.
    let project = store
        .config
        .name
        .as_deref()
        .map(|n| search::stem(&n.to_lowercase()));
    let mut words = search::query_terms(query);
    words.retain(|w| Some(w) != project.as_ref());
    let q = topic_terms(spelled(words, &df), &df, n);
    let base = bm25(&docs, &q);
    let expanded = expansion(store, &q, &df, n);
    let by_expansion: std::collections::HashMap<String, f64> = {
        let ranked = bm25(&docs, &expanded);
        let top = ranked.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
        ranked
            .into_iter()
            .filter(|(_, s)| *s > 0.0 && top > 0.0)
            .map(|(id, s)| (id, s / top))
            .collect()
    };
    let q_set: BTreeSet<&str> = q.iter().map(|t| t.as_str()).collect();
    let toks = tag_tokens(query);
    let missed = missed_for(store, &q, &df, n, own);
    let uncommon: BTreeSet<&str> = q
        .iter()
        .map(|t| t.as_str())
        .filter(|t| {
            let d = df.get(t).copied().unwrap_or(0);
            d > 0 && d as f64 / n <= COMMON_SHARE
        })
        .collect();
    let pairs = search::query_pairs(query);
    let docs_by_id: std::collections::HashMap<&str, &Vec<String>> =
        docs.iter().map(|(id, toks)| (id.as_str(), toks)).collect();
    let max_bm = base.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
    // Each clause of the task scored on its own and normalised to its own best match: a task
    // with four subjects gets the best rules of each, not four times the rules of whichever
    // subject has the most of them.
    let mut by_clause: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let clauses = search::clauses(query);
    if clauses.len() > 1 {
        for clause in &clauses {
            let cq = topic_terms(search::query_terms(clause), &df, n);
            let ranked = bm25(&docs, &cq);
            let top = ranked.iter().map(|(_, s)| *s).fold(0.0f64, f64::max);
            if top <= 0.0 {
                continue;
            }
            for (id, sc) in ranked {
                let v = by_clause.entry(id).or_insert(0.0);
                *v = v.max(sc / top);
            }
        }
    }
    let max_cite = rules
        .iter()
        .map(|r| cites.get(&r.id_str()).copied().unwrap_or(0))
        .max()
        .unwrap_or(0) as f64;
    let today = util::today_utc();
    let mut scored: Vec<Ranked> = base
        .into_iter()
        .filter_map(|(id, bm)| {
            let rec = store.get(&id)?;
            let whole = if max_bm > 0.0 { bm / max_bm } else { 0.0 };
            let lexical = match by_clause.get(&id) {
                Some(c) => CLAUSE_WEIGHT * c + (1.0 - CLAUSE_WEIGHT) * whole,
                None => whole,
            };
            // The words the task's tags lead to, at a weight that cannot carry a rule alone
            // past one the task's own words found.
            let expanded = EXPAND_WEIGHT * by_expansion.get(&id).copied().unwrap_or(0.0);
            let lexical = lexical + expanded;
            let shared = docs_by_id
                .get(id.as_str())
                .map(|toks| {
                    toks.iter()
                        .filter(|t| q_set.contains(t.as_str()))
                        .collect::<BTreeSet<_>>()
                        .len()
                })
                .unwrap_or(0);
            let area = rec.areas().iter().any(|a| areas.iter().any(|b| a == b));
            let cited = cites.get(&id).copied().unwrap_or(0);
            let cite_n = if max_cite > 0.0 {
                (1.0 + cited as f64).ln() / (1.0 + max_cite).ln()
            } else {
                0.0
            };
            let recent = if within_90(rec.date(), &today) {
                0.25
            } else {
                0.0
            };
            let title: BTreeSet<String> = search::terms(&slug_words(store, rec.title()))
                .into_iter()
                .collect();
            let titled = uncommon.iter().filter(|t| title.contains(**t)).count();
            let title_bonus = TITLE_BONUS * titled.saturating_sub(1).min(TITLE_MAX) as f64;
            let paired = docs_by_id
                .get(id.as_str())
                .map(|toks| {
                    pairs
                        .iter()
                        .filter(|(a, b)| toks.windows(2).any(|w| &w[0] == a && &w[1] == b))
                        .count()
                })
                .unwrap_or(0);
            let pair_bonus = PAIR_BONUS * paired.min(PAIR_MAX) as f64;
            let pin = if rec.pinned() { 1_000.0 } else { 0.0 };
            // A platform or an area as a rule's tag names every task of it (a rule tagged
            // `web` showed up on a deploy): the area has its own bonus, and a tag is a
            // situation.
            let tagged: Vec<String> = tags_named(rec, &toks, &[], &[], false)
                .into_iter()
                .filter(|t| !store.config.areas.contains_key(t) && !PLATFORMS.contains(&t.as_str()))
                .collect();
            let tag_bonus = TAG_BONUS * tagged.len().min(TAG_MAX) as f64;
            // A miss is a loop's note that this kind of task needs the rule. The task must
            // also name the rule — a tag, or its words — or likeness alone carried that
            // loop's traps into a deploy whose brief happened to share a few scope words.
            let hooked = !tagged.is_empty() || lexical >= MISS_HOOK;
            let miss = missed.get(&id).cloned().filter(|_| hooked);
            let miss_bonus = miss
                .as_ref()
                .map(|(_, alike)| {
                    MISS_BONUS * ((alike - MISS_FLOOR) / (MISS_FULL - MISS_FLOOR)).clamp(0.0, 1.0)
                })
                .unwrap_or(0.0);
            let termed = pointed.get(&id).cloned().unwrap_or_default();
            let term_bonus = TERM_BONUS * termed.len().min(TERM_MAX) as f64;
            let score = if lexical > 0.0
                || rec.pinned()
                || tag_bonus > 0.0
                || miss_bonus > 0.0
                || term_bonus > 0.0
            {
                3.0 * lexical
                    + term_bonus
                    + title_bonus
                    + pair_bonus
                    + tag_bonus
                    + miss_bonus
                    + if area { 1.0 } else { 0.0 }
                    + cite_n
                    + recent
                    + pin
            } else {
                0.0
            };
            Some(Ranked {
                id,
                score,
                lexical,
                area,
                cited,
                titled,
                paired,
                anchor: false,
                tagged,
                missed: miss,
                shared,
                expanded,
                termed,
            })
        })
        .filter(|r| r.score > 0.0)
        .collect();
    let by_score = |a: &Ranked, b: &Ranked| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    };
    scored.sort_by(by_score);
    if !areas.is_empty() {
        // An anchor shares a word of the task in its title: being famous in the area is not
        // enough, it has to be about something the task names.
        let q: BTreeSet<String> = q.iter().cloned().collect();
        let titled = |id: &str| {
            store
                .get(id)
                .map(|r| {
                    search::terms(&slug_words(store, r.title()))
                        .iter()
                        .any(|t| q.contains(t))
                })
                .unwrap_or(false)
        };
        let mut anchors: Vec<usize> = (0..scored.len())
            .filter(|&i| scored[i].area && scored[i].lexical >= 0.2 && titled(&scored[i].id))
            .collect();
        anchors.sort_by(|&a, &b| {
            scored[b]
                .cited
                .cmp(&scored[a].cited)
                .then(scored[a].id.cmp(&scored[b].id))
        });
        anchors.truncate(ANCHORS);
        for &i in &anchors {
            scored[i].anchor = true;
        }
        scored.sort_by(|a, b| b.anchor.cmp(&a.anchor).then(by_score(a, b)));
    }
    // Weak: the store holds none of the task's words; or no record holds two of them
    // and none has one in its title.
    let held = q_set.iter().filter(|t| df.contains_key(*t)).count();
    let best = scored.iter().map(|r| r.shared).max().unwrap_or(0);
    let in_a_title = || {
        scored.iter().any(|r| {
            store
                .get(&r.id)
                .map(|rec| {
                    search::terms(&slug_words(store, rec.title()))
                        .iter()
                        .any(|t| q_set.contains(t.as_str()))
                })
                .unwrap_or(false)
        })
    };
    let weak = !q_set.is_empty()
        && (held == 0 || (best < held.min(2) && !in_a_title()))
        && !scored.iter().any(|r| !r.termed.is_empty());
    (scored, weak)
}

fn rule_part_text(rec: &Record) -> Option<String> {
    record::find_part(&record::parts_of(rec), "rule").map(|p| p.text.clone())
}

fn never_whole(store: &Store, rel: &str) -> bool {
    let full = format!("{}/{}", store.config.store_rel, rel);
    store
        .config
        .never_whole
        .iter()
        .any(|p| p == rel || p == &full || crate::util::glob_match(p, &full))
}

fn within_90(date: &str, today: &str) -> bool {
    let Some(d) = date_ord(date) else {
        return false;
    };
    let Some(t) = date_ord(today) else {
        return false;
    };
    t >= d && t - d <= 90
}

fn date_ord(s: &str) -> Option<i64> {
    if s.len() < 10 || s == "unknown" {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe as i64 - 719468)
}

/// A chunk cut at a line boundary to about `cap` tok4, with a pointer to the whole record.
fn cut_chunk(chunk: &Chunk, cap: usize) -> Chunk {
    let copy = Chunk {
        id: chunk.id.clone(),
        text: chunk.text.clone(),
        also: chunk.also.clone(),
    };
    if tok4(&chunk.text) <= cap {
        return copy;
    }
    let note = format!(
        "… (cut to fit; `trailmark show {}` has the rest)\n",
        chunk.id
    );
    let room = cap.saturating_sub(tok4(&note)) * 4;
    let mut text = String::new();
    for line in chunk.text.split_inclusive('\n') {
        if text.chars().count() + line.chars().count() > room {
            break;
        }
        text.push_str(line);
    }
    text.push_str(&note);
    Chunk { text, ..copy }
}

/// The conventions printed in full.
const FULL_CAP: usize = 5;
/// Shares of the pool, in percent: target, open items, recipes, facts, full text, related,
/// one line each (the last takes what is left and what the others did not use).
const WEIGHTS: [usize; 7] = [20, 10, 12, 8, 10, 8, 32];
/// The best matches the related records are reached from, each with at least this word score
/// or a tag the task says.
const SEEDS: usize = 12;
const SEED_WORDS: f64 = 0.3;
const RELATED_CAP: usize = 16;
/// How many nearest conventions a weak match lists.
const DID_YOU_MEAN: usize = 5;
/// A line of a list is cut here: an id and enough of its title to choose it.
const LINE_CHARS: usize = 120;
/// The word score below which a record without a tag or a miss is not listed (see `build`).
const LINE_FLOOR: f64 = 0.15;

/// One line cut at `max` characters, on a word, with an ellipsis.
fn cut_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() <= max {
        return line.to_string();
    }
    let cut: String = line.chars().take(max).collect();
    let at = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

fn take_until(chunks: &[Chunk], room: usize) -> (Vec<Chunk>, usize) {
    let mut keep = Vec::new();
    let mut used = 0usize;
    for chunk in chunks {
        let n = tok4(&chunk.text);
        if used + n > room && !keep.is_empty() {
            break;
        }
        if n > room && keep.is_empty() {
            break;
        }
        used += n;
        keep.push(Chunk {
            id: chunk.id.clone(),
            text: chunk.text.clone(),
            also: chunk.also.clone(),
        });
    }
    (keep, used)
}

fn assemble(label: &str, core: &str, kept: &[Vec<Chunk>], tail: &str) -> String {
    let heads = [
        "## Your target\n\n",
        "## Open in its areas\n\n",
        "## How to run\n\n",
        "## Gates and traps\n\n",
        "## Rules — full text\n\n",
        "## Related — one hop from the best matches (trailmark show <id>)\n\n",
        "## Rules — one line each (trailmark show <id>)\n\n",
    ];
    // The core carries a header comment; keep its markdown from the first heading.
    let core_md = core.find("# ").map(|pos| &core[pos..]).unwrap_or(core);
    let mut body = format!("# Context — {label}\n\n{}\n\n", core_md.trim_end());
    for (i, (head, chunks)) in heads.iter().zip(kept).enumerate() {
        if chunks.is_empty() {
            continue;
        }
        let did_you_mean = i == 4 && chunks.iter().any(|c| c.id == "did-you-mean");
        body.push_str(if did_you_mean {
            "## Did you mean\n\n"
        } else {
            head
        });
        // Open items, related and one-line rules are lists: one line each, no blank line between.
        let list = matches!(i, 1 | 5 | 6);
        for chunk in chunks {
            body.push_str(chunk.text.trim_end());
            body.push_str(if list { "\n" } else { "\n\n" });
        }
        if list {
            body.push('\n');
        }
    }
    body.push_str(tail);
    body
}

fn finish_bundle(label: &str, body: &str, budget: usize) -> String {
    let mut tok = 0usize;
    let mut full = String::new();
    for _ in 0..8 {
        let header =
            format!("<!-- trailmark:context target={label} budget={budget} tok4={tok} -->\n");
        full = format!("{header}{body}");
        let n = tok4(&full);
        if n == tok {
            return full;
        }
        tok = n;
    }
    full
}

/// A part with nothing in it but its heading and `Pending.` (what `add` writes for a part it
/// was not given).
fn pending(part: &str) -> bool {
    let body: String = part
        .lines()
        .filter(|l| !l.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n");
    let t = body.trim();
    t.is_empty() || t == "Pending."
}

fn part_text(rec: &Record, name: &str) -> String {
    record::find_part(&record::parts_of(rec), name)
        .map(|p| p.text.clone())
        .unwrap_or_default()
}

fn one(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
