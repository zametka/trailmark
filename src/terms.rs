//! The term index: the project's words for its conventions, the words a newcomer says
//! instead, and the records each term leads to. An agent reads `trailmark terms`, rewrites its task
//! in those terms, and asks `trailmark context --terms '<term>, <term>' "<task>"`; the bundle then
//! favours the records the named terms point to, and ranks on the terms' words as well as the
//! task's. The point of the index is to translate the task into the project's words first.
//!
//! The file is `terms.toml` in the store (or `TRAIL_TERMS_FILE`, which the evaluation uses for
//! its overlays and its placebo):
//!
//! ```toml
//! [[term]]
//! name = "worktree stack"
//! area = "tests"
//! says = ["second copy of the app", "run e2e tests locally"]
//! ids = ["FACT-recipe-worktree-stack", "RULE-1"]
//! ```

use crate::store::Store;
use crate::util::tok4;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub struct Term {
    pub name: String,
    pub area: String,
    pub says: Vec<String>,
    pub ids: Vec<String>,
}

/// What a `--terms` string resolved to.
#[derive(Default)]
pub struct Resolved {
    /// The index's names of the terms given, in the order given, each once.
    pub names: Vec<String>,
    /// Record id -> the names of the given terms that point to it.
    pub pointed: BTreeMap<String, Vec<String>>,
    /// Words given that name no term.
    pub unknown: Vec<String>,
}

pub fn path(store: &Store) -> PathBuf {
    match std::env::var_os("TRAIL_TERMS_FILE") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => store.config.store_dir(&store.root).join("terms.toml"),
    }
}

/// The store's terms; none when the file is absent.
pub fn load(store: &Store) -> Result<Vec<Term>, String> {
    let p = path(store);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    parse(&text).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn parse(text: &str) -> Result<Vec<Term>, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let strings = |t: &toml::Value, k: &str| -> Vec<String> {
        t.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (i, t) in v
        .get("term")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = t
            .get("name")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(format!("term {} has no name", i + 1))?;
        if name.contains([',', ';']) {
            return Err(format!("term '{name}': a name holds no comma or semicolon"));
        }
        out.push(Term {
            area: t
                .get("area")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            says: strings(t, "says"),
            ids: strings(t, "ids"),
            name,
        });
    }
    Ok(out)
}

fn fold(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Resolve `--terms 'a, b; c'`: each piece is a term's name, or one of its phrases, matched
/// without case. Only ids the store holds are pointed to.
pub fn resolve(store: &Store, terms: &[Term], given: &str) -> Resolved {
    let mut out = Resolved::default();
    for piece in given.split([',', ';', '\n']) {
        let want = fold(piece.trim().trim_matches(['"', '\'', '`']));
        if want.is_empty() {
            continue;
        }
        let hit = terms.iter().find(|t| fold(&t.name) == want).or_else(|| {
            terms
                .iter()
                .find(|t| t.says.iter().any(|s| fold(s) == want))
        });
        match hit {
            Some(t) => {
                if out.names.contains(&t.name) {
                    continue;
                }
                out.names.push(t.name.clone());
                for id in &t.ids {
                    if store.get(id).is_some() {
                        out.pointed
                            .entry(id.clone())
                            .or_default()
                            .push(t.name.clone());
                    }
                }
            }
            None => {
                if !out.unknown.contains(&want) {
                    out.unknown.push(want)
                }
            }
        }
    }
    out
}

/// The line a bundle's target carries: which terms were read, and which words named none.
pub fn target_line(r: &Resolved) -> Option<String> {
    if r.names.is_empty() && r.unknown.is_empty() {
        return None;
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for names in r.pointed.values() {
        for n in names {
            *counts.entry(n.as_str()).or_default() += 1;
        }
    }
    let mut text = String::from("Terms: ");
    text.push_str(
        &r.names
            .iter()
            .map(|n| format!("{n} ({})", counts.get(n.as_str()).copied().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join(" · "),
    );
    if !r.unknown.is_empty() {
        if !r.names.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&format!(
            "not in the index: {} (`trailmark terms` lists the terms)",
            r.unknown.join(", ")
        ));
    }
    text.push('\n');
    Some(text)
}

/// A section-style rule id is printed without its prefix (`LESSON-2.4` as `2.4`), which is how
/// that numbering is cited. A number-style id is printed whole.
fn short_id(id: &str) -> String {
    let scheme = crate::id::scheme();
    if scheme.section_rules() {
        let prefix = format!("{}-", scheme.rule);
        if let Some(rest) = id.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    id.to_string()
}

pub struct Printed {
    pub text: String,
    /// The ids the store does not hold (left out of the print).
    pub dead: Vec<String>,
}

/// `trailmark terms`: the whole index within `budget` tok4. Each term prints its phrases and its
/// ids; when the whole does not fit, every term prints fewer of each (phrases first, never
/// below two of each), and the last line says how to see the rest. `all` prints everything.
/// `filter`: only terms whose name, phrases or area hold these words.
pub fn render(store: &Store, terms: &[Term], budget: usize, all: bool, filter: &str) -> Printed {
    let mut dead = BTreeSet::new();
    let live: Vec<(&Term, Vec<&str>)> = terms
        .iter()
        .filter(|t| {
            let want = fold(filter);
            want.is_empty()
                || want.split(' ').all(|w| {
                    fold(&t.name).contains(w)
                        || fold(&t.area).contains(w)
                        || t.says.iter().any(|s| fold(s).contains(w))
                })
        })
        .map(|t| {
            let ids = t
                .ids
                .iter()
                .filter(|id| {
                    let ok = store.get(id).is_some();
                    if !ok {
                        dead.insert((*id).clone());
                    }
                    ok
                })
                .map(|s| s.as_str())
                .collect();
            (t, ids)
        })
        .collect();
    let draw = |says_max: usize, ids_max: usize| -> String {
        let mut body = String::new();
        let mut area: Option<&str> = None;
        let mut order: Vec<&(&Term, Vec<&str>)> = live.iter().collect();
        // Grouped by area, in the order each area first appears; terms keep the file's order.
        let mut first: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, (t, _)) in live.iter().enumerate() {
            first.entry(t.area.as_str()).or_insert(i);
        }
        order.sort_by_key(|(t, _)| first.get(t.area.as_str()).copied().unwrap_or(0));
        for (t, ids) in order {
            if area != Some(t.area.as_str()) {
                area = Some(t.area.as_str());
                if !t.area.is_empty() {
                    body.push_str(&format!("\n## {}\n", t.area));
                }
            }
            let says: Vec<&str> = t.says.iter().take(says_max).map(|s| s.as_str()).collect();
            let shown: Vec<String> = ids.iter().take(ids_max).map(|i| short_id(i)).collect();
            let more = ids.len().saturating_sub(ids_max);
            body.push_str(&t.name);
            if !says.is_empty() {
                body.push_str(": ");
                body.push_str(&says.join(" · "));
            }
            if !shown.is_empty() {
                body.push_str(" → ");
                body.push_str(&shown.join(" "));
                if more > 0 {
                    body.push_str(&format!(" +{more}"));
                }
            }
            body.push('\n');
        }
        body
    };
    let head = |n: usize, cut: bool| -> String {
        format!(
            "# Terms — the project's words, and what you may call them instead ({n} terms)\n\
             Rewrite your task in these terms, then: trailmark context --terms '<term>, <term>' \"<task>\"\n\
             A name or a phrase after it may be given; ids after → are the records a term leads to.{}\n",
            if cut {
                " Cut to fit: `trailmark terms --all` prints every phrase and id, `trailmark terms <word>` one subject."
            } else {
                ""
            }
        )
    };
    let wrap = |head: String, body: String| -> String {
        let inner = format!("{head}{body}");
        format!("<!-- trailmark:terms tok4={} -->\n{inner}", tok4(&inner))
    };
    let full = wrap(head(live.len(), false), draw(usize::MAX, usize::MAX));
    if all || tok4(&full) <= budget {
        return Printed {
            text: full,
            dead: dead.into_iter().collect(),
        };
    }
    let mut steps = Vec::new();
    for s in (2..=5).rev() {
        for i in (2..=6).rev() {
            steps.push((s, i));
        }
    }
    steps.sort_by(|a, b| (b.0 + b.1).cmp(&(a.0 + a.1)).then(b.1.cmp(&a.1)));
    let mut text = String::new();
    for (s, i) in steps {
        text = wrap(head(live.len(), true), draw(s, i));
        if tok4(&text) <= budget {
            break;
        }
    }
    Printed {
        text,
        dead: dead.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_with_a_comma_is_refused_and_an_empty_file_has_no_terms() {
        assert!(parse("[[term]]\nname = \"a, b\"\n").is_err());
        assert!(parse("[[term]]\nsays = [\"x\"]\n").is_err());
        assert!(parse("").unwrap().is_empty());
        let t = parse(
            "[[term]]\nname = \" land on main \"\nsays = [\"merge\", \"\"]\nids = [\"RULE-1\"]\n",
        )
        .unwrap();
        assert_eq!(t[0].name, "land on main");
        assert_eq!(t[0].says, ["merge"]);
    }

    #[test]
    fn short_ids_keep_every_other_prefix() {
        assert_eq!(short_id("RULE-12"), "RULE-12");
        let _guard = crate::id::push_scheme(crate::id::IdScheme {
            rule: "LESSON".into(),
            rule_style: "section".into(),
            ..crate::id::IdScheme::default()
        });
        assert_eq!(short_id("LESSON-2.7"), "2.7");
        assert_eq!(
            short_id("FACT-recipe-app-worktree"),
            "FACT-recipe-app-worktree"
        );
        assert_eq!(short_id("DEC-0007"), "DEC-0007");
    }
}
