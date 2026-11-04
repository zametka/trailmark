//! The records one hop from a bundle's best matches: what the task's convention links,
//! cites, is cited by, was missed with, or was learned with. A task names the convention in
//! its own words; the records around that convention rarely share those words, and they are
//! the ids an agent would otherwise go and find by hand.

use crate::record::{is_tombstone, Record};
use crate::scan;
use crate::store::Store;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How strongly one record points at another, by the kind of edge.
const LINKED: f64 = 1.0; // a link field: links, supersedes, narrows, either, follows, decisions
const CITES: f64 = 0.8; // the seed's own text names it
const CITED_BY: f64 = 0.5; // its text names the seed
const MISSED_WITH: f64 = 0.6; // a loop needed both and its bundle had neither (per loop, up to 2)
const LEARNED_WITH: f64 = 0.4; // one loop wrote both
const FILE: f64 = 1.0; // it links a file the task names

/// A neighbour scores at least this much: a seed's weight (0..1) times its edge's strength,
/// summed over the seeds. Below it, nothing a held-out brief needed was ever listed: 174 lines
/// of 675 on 25 briefs read twice (literal, paraphrased), no hit.
const MIN_SCORE: f64 = 0.5;

const LINK_FIELDS: &[&str] = &[
    "links",
    "supersedes",
    "superseded_by",
    "narrows",
    "narrowed_by",
    "either",
    "follows",
    "decisions",
];

pub struct Near {
    pub id: String,
    pub score: f64,
    /// The seed it was reached from (the strongest), or the file the task names.
    pub via: String,
}

/// A record that can be a neighbour: a convention or a fact, live.
fn eligible(rec: &Record) -> bool {
    matches!(rec.type_name(), "rule" | "decision" | "fact")
        && !is_tombstone(rec.status())
        && rec.status() != "superseded"
}

fn add(out: &mut HashMap<String, Vec<(String, f64)>>, from: &str, to: &str, w: f64) {
    if from != to {
        out.entry(from.to_string())
            .or_default()
            .push((to.to_string(), w));
    }
}

/// Every edge of the store between conventions and facts, both directions where the edge is
/// symmetric. Built per bundle: about 2,000 records scan in a few milliseconds.
fn edges(store: &Store) -> HashMap<String, Vec<(String, f64)>> {
    let mut out: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    let live = |id: &str| store.get(id).filter(|r| eligible(r)).is_some();
    let mut missed_by_loop: Vec<Vec<String>> = Vec::new();
    for rec in &store.records {
        let id = rec.id_str();
        match rec.type_name() {
            "loop" | "review" => {
                let missed: Vec<String> = rec
                    .fm
                    .strings("missed")
                    .into_iter()
                    .filter(|m| live(m))
                    .collect();
                if missed.len() > 1 {
                    missed_by_loop.push(missed);
                }
                let learned: Vec<String> = rec
                    .fm
                    .strings("learned")
                    .into_iter()
                    .filter(|m| live(m))
                    .collect();
                for a in &learned {
                    for b in &learned {
                        add(&mut out, a, b, LEARNED_WITH);
                    }
                }
            }
            "rule" | "decision" | "fact" if !is_tombstone(rec.status()) => {
                for key in LINK_FIELDS {
                    for to in rec.fm.strings(key) {
                        if live(&to) {
                            add(&mut out, &id, &to, LINKED);
                            add(&mut out, &to, &id, LINKED);
                        }
                    }
                }
                let mut cited = BTreeSet::new();
                for hit in scan::body_hits(store, rec) {
                    if hit.ambiguous {
                        continue;
                    }
                    for to in hit.ids {
                        if live(&to) && cited.insert(to.clone()) {
                            add(&mut out, &id, &to, CITES);
                            add(&mut out, &to, &id, CITED_BY);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Two records one loop missed together, counted per loop up to twice.
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    for missed in &missed_by_loop {
        for a in missed {
            for b in missed {
                if a != b {
                    *pairs.entry((a.clone(), b.clone())).or_insert(0) += 1;
                }
            }
        }
    }
    for ((a, b), n) in pairs {
        add(&mut out, &a, &b, MISSED_WITH * n.min(2) as f64);
    }
    out
}

/// The task's file names: a word with a slash or a dot and a known-looking extension.
fn named_files(task: &str) -> Vec<String> {
    task.split(|c: char| c.is_whitespace() || matches!(c, '`' | '(' | ')' | ',' | ';' | '"' | '\''))
        .map(|w| w.trim_end_matches(['.', ':']))
        .filter(|w| {
            w.len() > 3
                && (w.contains('/')
                    || w.rsplit_once('.').is_some_and(|(stem, ext)| {
                        !stem.is_empty()
                            && (1..=4).contains(&ext.len())
                            && ext.chars().all(|c| c.is_ascii_alphabetic())
                    }))
        })
        .map(|w| w.to_string())
        .collect()
}

/// The records one hop from `seeds` (id, weight 0..1), best first, without `skip`. A neighbour
/// reached from one seed only, by a citation or a miss, must hold two of the task's words in
/// its title (`task_terms`, stemmed): one shared citation is how a narrow rule would reach a
/// deploy bundle. A link field, or two seeds, is
/// enough on its own.
pub fn related(
    store: &Store,
    seeds: &[(String, f64)],
    skip: &BTreeSet<String>,
    task: &str,
    task_terms: &BTreeSet<String>,
    cap: usize,
) -> Vec<Near> {
    if cap == 0 {
        return Vec::new();
    }
    let graph = edges(store);
    // id -> (score, seeds reached from, best seed, best edge, strongest single contribution)
    let mut acc: HashMap<String, (f64, BTreeSet<String>, String, f64, f64)> = HashMap::new();
    for (seed, w) in seeds {
        let Some(list) = graph.get(seed) else {
            continue;
        };
        // One edge per neighbour per seed: the strongest.
        let mut best: BTreeMap<&str, f64> = BTreeMap::new();
        for (to, ew) in list {
            let e = best.entry(to.as_str()).or_insert(0.0);
            *e = e.max(*ew);
        }
        for (to, ew) in best {
            let gain = w * ew;
            let entry = acc.entry(to.to_string()).or_insert((
                0.0,
                BTreeSet::new(),
                String::new(),
                0.0,
                0.0,
            ));
            entry.0 += gain;
            entry.1.insert(seed.clone());
            if gain > entry.4 {
                entry.2 = seed.clone();
                entry.3 = ew;
                entry.4 = gain;
            }
        }
    }
    let files = named_files(task);
    if !files.is_empty() {
        for rec in store.records.iter().filter(|r| eligible(r)) {
            for link in rec.fm.strings("links") {
                if let Some(f) = files
                    .iter()
                    .find(|f| link == **f || link.ends_with(&format!("/{f}")))
                {
                    let entry = acc.entry(rec.id_str()).or_insert((
                        0.0,
                        BTreeSet::new(),
                        String::new(),
                        0.0,
                        0.0,
                    ));
                    entry.0 += FILE;
                    entry.1.insert(format!("file:{f}"));
                    entry.2 = f.clone();
                    entry.3 = LINKED;
                    entry.4 = FILE;
                }
            }
        }
    }
    let seed_ids: BTreeSet<&str> = seeds.iter().map(|(s, _)| s.as_str()).collect();
    let mut out: Vec<Near> = acc
        .into_iter()
        .filter(|(id, _)| !skip.contains(id) && !seed_ids.contains(id.as_str()))
        .filter(|(_, (score, ..))| *score >= MIN_SCORE)
        .filter(|(id, (_, from, _, edge, _))| {
            if from.len() >= 2 || *edge >= LINKED {
                return true;
            }
            // Reached once, by a citation or a miss: its title must hold two of the task's
            // words. One shared word (a stem of an unrelated subject) brought a recipe
            // into a bundle about something else.
            store
                .get(id)
                .map(|r| {
                    crate::search::terms(&crate::context::slug_words(store, r.title()))
                        .iter()
                        .filter(|t| task_terms.contains(*t))
                        .collect::<BTreeSet<_>>()
                        .len()
                        >= 2
                })
                .unwrap_or(false)
        })
        .map(|(id, (score, _, via, _, _))| Near { id, score, via })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out.truncate(cap);
    out
}

#[cfg(test)]
mod tests {
    use super::named_files;

    #[test]
    fn a_task_names_files_by_path_or_extension() {
        let f =
            named_files("fix `web/src/table-model.ts` and the label in app.yml, then e.g. ship");
        assert_eq!(f, vec!["web/src/table-model.ts", "app.yml"]);
        assert!(named_files("a sentence. Another one").is_empty());
    }
}
