//! BM25 over plain tokens. No embeddings, no network.
//! k1 = 1.2, b = 0.75, idf = ln(1 + (n - df + 0.5) / (df + 0.5)).

use std::collections::{BTreeSet, HashMap};

pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Words with their stem (`stem`), for ranking: "overflows" and "overflow" are one term.
pub fn terms(text: &str) -> Vec<String> {
    tokenize(text).into_iter().map(|t| stem(&t)).collect()
}

/// Terms of a query, without words that carry no topic ("fix", "the", "at").
pub fn query_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tokenize(text) {
        if STOP.contains(&t.as_str()) {
            continue;
        }
        let s = stem(&t);
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// True for a word that carries no topic (the list below).
pub fn is_stop(word: &str) -> bool {
    STOP.contains(&word)
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "does", "fix", "for", "from", "how",
    "in", "into", "is", "it", "its", "make", "of", "on", "or", "so", "that", "the", "their",
    "then", "this", "to", "when", "with", "what", "why", "we", "our", "should", "can", "not",
    // The prefix of a hyphenated word ("re-render", "re-read") is split off as a word of its
    // own and matched every rule that re-does anything.
    "re",
];

/// Adjacent word pairs of a task, within one clause, neither a stop word ("memory leak",
/// "long session", "re render"). A pair names a thing where its two words alone are
/// homonyms: a rule about a session cookie and a rule about data that leaks both share
/// "session" and "leak" with "memory leak in a long session", and neither holds the pair.
pub fn query_pairs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for clause in text.split(is_clause_break) {
        let toks = tokenize(clause);
        for w in toks.windows(2) {
            if STOP.contains(&w[0].as_str()) || STOP.contains(&w[1].as_str()) {
                continue;
            }
            let pair = (stem(&w[0]), stem(&w[1]));
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

/// The clauses of a task: its parts between commas, semicolons, colons, full stops and line
/// ends, each with at least one word that is not a stop word.
pub fn clauses(text: &str) -> Vec<String> {
    text.split(is_clause_break)
        .map(|c| c.trim().to_string())
        .filter(|c| tokenize(c).iter().any(|t| !STOP.contains(&t.as_str())))
        .collect()
}

fn is_clause_break(c: char) -> bool {
    matches!(c, ',' | ';' | ':' | '.' | '\n' | '(' | ')')
}

/// Words with the plural-and-tense stem only (`stem_light`): what an area's keyword is
/// matched against. Porter's stem joins "tokenizer" to "token", and a layout task became a
/// security task. Matching whole words kept "token" out of "tokenizer".
pub fn terms_light(text: &str) -> Vec<String> {
    tokenize(text).into_iter().map(|t| stem_light(&t)).collect()
}

/// True when `phrase` (one or more words) occurs in `terms_light` words as whole words, in order.
/// A keyword is matched as words, never as a substring: "auth" is not in "author", "spec"
/// is not in "inspect", "route" is not in "router" (substring matching put every task
/// that names the author into the security area).
pub fn has_phrase(terms: &[String], phrase: &str) -> bool {
    let want = terms_light(phrase);
    if want.is_empty() || want.len() > terms.len() {
        return false;
    }
    terms.windows(want.len()).any(|w| w == want.as_slice())
}

/// The light stem: plural and -ing/-ed endings only. An older stem; still the
/// one an area's keywords are matched with (`terms_light`).
pub fn stem_light(word: &str) -> String {
    let w = word;
    let n = w.chars().count();
    if !w.is_ascii() || n <= 3 {
        return w.to_string();
    }
    if let Some(base) = w.strip_suffix("ies") {
        if n > 4 {
            return format!("{base}y");
        }
    }
    if let Some(base) = w.strip_suffix("ing") {
        if n > 5 {
            return base.to_string();
        }
    }
    if let Some(base) = w.strip_suffix("ed") {
        if n > 4 {
            return base.to_string();
        }
    }
    if w.ends_with("ss") || w.ends_with("us") || w.ends_with("is") {
        return w.to_string();
    }
    if let Some(base) = w.strip_suffix("es") {
        if base.ends_with("sh") || base.ends_with("ch") || base.ends_with('x') {
            return base.to_string();
        }
    }
    if let Some(base) = w.strip_suffix('s') {
        return base.to_string();
    }
    w.to_string()
}

/// A word's stem for ranking: Porter's algorithm (1980), so a paraphrase that says
/// "translation" finds the rule that says "translate", "deployment" the one that says
/// "deploy", "measured" the one that says "measurement" (; the plural-and-tense stem
/// before it joined only the first and the last of those). Words of three letters or fewer,
/// words with a digit and words outside ASCII are kept as they are: `h1`, `12`, `ç`.
pub fn stem(word: &str) -> String {
    if !word.is_ascii() || word.len() <= 3 || word.bytes().any(|b| !b.is_ascii_lowercase()) {
        return word.to_string();
    }
    porter::stem(word)
}

/// Porter, M. F. "An algorithm for suffix stripping", Program 14(3), 1980: steps 1a to 5b.
mod porter {
    fn cons(w: &[u8], i: usize) -> bool {
        match w[i] {
            b'a' | b'e' | b'i' | b'o' | b'u' => false,
            b'y' => i == 0 || !cons(w, i - 1),
            _ => true,
        }
    }

    /// The number of vowel-consonant sequences in `w[..len]`.
    fn measure(w: &[u8], len: usize) -> usize {
        let mut m = 0;
        let mut i = 0;
        while i < len && cons(w, i) {
            i += 1;
        }
        while i < len {
            while i < len && !cons(w, i) {
                i += 1;
            }
            if i >= len {
                break;
            }
            while i < len && cons(w, i) {
                i += 1;
            }
            m += 1;
        }
        m
    }

    fn has_vowel(w: &[u8], len: usize) -> bool {
        (0..len).any(|i| !cons(w, i))
    }

    fn double_cons(w: &[u8], len: usize) -> bool {
        len >= 2 && w[len - 1] == w[len - 2] && cons(w, len - 1)
    }

    /// consonant-vowel-consonant at the end, the last not w, x or y ("hop", not "show").
    fn cvc(w: &[u8], len: usize) -> bool {
        len >= 3
            && cons(w, len - 3)
            && !cons(w, len - 2)
            && cons(w, len - 1)
            && !matches!(w[len - 1], b'w' | b'x' | b'y')
    }

    /// Replace `suffix` by `with` when the stem before it has a measure above `min`.
    fn swap(w: &mut Vec<u8>, suffix: &str, with: &str, min: usize) -> bool {
        if !w.ends_with(suffix.as_bytes()) {
            return false;
        }
        let stem = w.len() - suffix.len();
        if measure(w, stem) > min {
            w.truncate(stem);
            w.extend_from_slice(with.as_bytes());
        }
        true
    }

    pub fn stem(word: &str) -> String {
        let mut w = word.as_bytes().to_vec();
        // 1a
        if w.ends_with(b"sses") || w.ends_with(b"ies") {
            w.truncate(w.len() - 2);
        } else if w.ends_with(b"ss") {
        } else if w.ends_with(b"s") {
            w.pop();
        }
        // 1b
        let mut again = false;
        if w.ends_with(b"eed") {
            if measure(&w, w.len() - 3) > 0 {
                w.pop();
            }
        } else {
            for suffix in [&b"ed"[..], &b"ing"[..]] {
                if w.ends_with(suffix) && has_vowel(&w, w.len() - suffix.len()) {
                    w.truncate(w.len() - suffix.len());
                    again = true;
                    break;
                }
            }
        }
        if again {
            if w.ends_with(b"at") || w.ends_with(b"bl") || w.ends_with(b"iz") {
                w.push(b'e');
            } else if double_cons(&w, w.len()) && !matches!(w[w.len() - 1], b'l' | b's' | b'z') {
                w.pop();
            } else if measure(&w, w.len()) == 1 && cvc(&w, w.len()) {
                w.push(b'e');
            }
        }
        // 1c, as Porter2 has it: y after a consonant that is not the first letter ("memory",
        // not "deploy", whose y the original turned into i only before an ending).
        if w.len() > 2 && w.ends_with(b"y") && cons(&w, w.len() - 2) {
            let n = w.len();
            w[n - 1] = b'i';
        }
        // 2
        const STEP2: &[(&str, &str)] = &[
            ("ational", "ate"),
            ("tional", "tion"),
            ("enci", "ence"),
            ("anci", "ance"),
            ("izer", "ize"),
            ("abli", "able"),
            ("alli", "al"),
            ("entli", "ent"),
            ("eli", "e"),
            ("ousli", "ous"),
            ("ization", "ize"),
            ("ation", "ate"),
            ("ator", "ate"),
            ("alism", "al"),
            ("iveness", "ive"),
            ("fulness", "ful"),
            ("ousness", "ous"),
            ("aliti", "al"),
            ("iviti", "ive"),
            ("biliti", "ble"),
        ];
        for (suffix, with) in STEP2 {
            if swap(&mut w, suffix, with, 0) {
                break;
            }
        }
        // 3
        const STEP3: &[(&str, &str)] = &[
            ("icate", "ic"),
            ("ative", ""),
            ("alize", "al"),
            ("iciti", "ic"),
            ("ical", "ic"),
            ("ful", ""),
            ("ness", ""),
        ];
        for (suffix, with) in STEP3 {
            if swap(&mut w, suffix, with, 0) {
                break;
            }
        }
        // 4
        const STEP4: &[&str] = &[
            "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent", "ion",
            "ou", "ism", "ate", "iti", "ous", "ive", "ize",
        ];
        // The longest suffix that matches is the one considered (Porter's table order).
        let mut best: Option<&str> = None;
        for suffix in STEP4 {
            if w.ends_with(suffix.as_bytes())
                && best.map(|b| suffix.len() > b.len()).unwrap_or(true)
            {
                best = Some(suffix);
            }
        }
        if let Some(suffix) = best {
            let stem = w.len() - suffix.len();
            let ok = if suffix == "ion" {
                stem > 0 && matches!(w[stem - 1], b's' | b't')
            } else {
                true
            };
            if ok && measure(&w, stem) > 1 {
                w.truncate(stem);
            }
        }
        // 5a
        if w.ends_with(b"e") {
            let stem = w.len() - 1;
            let m = measure(&w, stem);
            if m > 1 || (m == 1 && !cvc(&w, stem)) {
                w.pop();
            }
        }
        // 5b
        if measure(&w, w.len()) > 1 && double_cons(&w, w.len()) && w.ends_with(b"l") {
            w.pop();
        }
        String::from_utf8(w).unwrap_or_else(|_| word.to_string())
    }
}

/// How many of `docs` hold each term.
pub fn doc_freq(docs: &[(String, Vec<String>)]) -> HashMap<&str, usize> {
    let mut df: HashMap<&str, usize> = HashMap::new();
    for (_, toks) in docs {
        let mut seen = BTreeSet::new();
        for t in toks {
            seen.insert(t.as_str());
        }
        for t in seen {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    df
}

/// Rank `docs` (id, tokens). Title weighting is the caller's job: repeat title tokens.
/// Ties break by id ascending.
pub fn bm25(docs: &[(String, Vec<String>)], query: &[String]) -> Vec<(String, f64)> {
    let n = docs.len() as f64;
    if n == 0.0 || query.is_empty() {
        return docs.iter().map(|(id, _)| (id.clone(), 0.0)).collect();
    }
    let avgdl = docs.iter().map(|(_, t)| t.len()).sum::<usize>() as f64 / n;
    let df = doc_freq(docs);
    let mut scored = Vec::with_capacity(docs.len());
    for (id, toks) in docs {
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in toks {
            *tf.entry(t.as_str()).or_insert(0) += 1;
        }
        let dl = toks.len() as f64;
        let mut score = 0.0;
        for q in query {
            let f = *tf.get(q.as_str()).unwrap_or(&0) as f64;
            if f == 0.0 {
                continue;
            }
            let dfi = *df.get(q.as_str()).unwrap_or(&0) as f64;
            let idf = (1.0 + (n - dfi + 0.5) / (dfi + 0.5)).ln();
            let k1 = 1.2;
            let b = 0.75;
            let denom = f + k1 * (1.0 - b + b * dl / avgdl.max(1e-9));
            score += idf * (f * (k1 + 1.0)) / denom;
        }
        scored.push((id.clone(), score));
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_is_whole_words() {
        let t = terms_light("the author asked for a spec of the router");
        assert!(!has_phrase(&t, "auth"));
        assert!(!has_phrase(&t, "route"));
        assert!(has_phrase(&t, "spec"));
        assert!(has_phrase(&terms_light("edit app.yml then"), "app.yml"));
        assert!(has_phrase(
            &terms_light("a React Native screen"),
            "react native"
        ));
        assert!(has_phrase(&terms_light("two deploys"), "deploy"));
        assert!(!has_phrase(&terms_light("the layout tokenizer"), "token"));
    }

    #[test]
    fn pairs_stay_in_their_clause() {
        let p = query_pairs("memory leak in a long session, hover flicker");
        assert!(p.contains(&(stem("memory"), stem("leak"))));
        assert!(p.contains(&(stem("long"), stem("session"))));
        assert!(!p.contains(&("session".into(), "hover".into())));
        assert!(!p.iter().any(|(a, b)| a == "in" || b == "a"));
    }

    #[test]
    fn a_paraphrase_meets_the_rule_on_its_stem() {
        //  the words a half-remembered task uses for the words a rule uses.
        for group in [
            &["translation", "translate", "translated", "translations"][..],
            &["deployment", "deploy", "deployed", "deploys"],
            &["measurement", "measured", "measure"],
            &["migration", "migrate", "migrating"],
            &["rendering", "renders", "rendered"],
            &["overflows", "overflow"],
            &["connection", "connections", "connected"],
        ] {
            let stems: Vec<String> = group.iter().map(|w| stem(w)).collect();
            assert!(
                stems.iter().all(|s| s == &stems[0]),
                "{group:?} -> {stems:?}"
            );
        }
        // Different words stay different, and short, numeric or non-ASCII words are kept.
        for (a, b) in [
            ("auth", "author"),
            ("route", "router"),
            ("spec", "special"),
            ("land", "landscape"),
        ] {
            assert_ne!(stem(a), stem(b), "{a} / {b}");
        }
        for w in ["h1", "12", "css", "café", "tok4", "url"] {
            assert_eq!(stem(w), w);
        }
        // The stem before joined only plurals and -ing/-ed.
        assert_ne!(stem_light("translation"), stem_light("translate"));
    }

    #[test]
    fn ties_break_by_id() {
        let docs = vec![
            ("B".into(), tokenize("alpha beta")),
            ("A".into(), tokenize("alpha beta")),
        ];
        let ranked = bm25(&docs, &tokenize("alpha"));
        assert_eq!(ranked[0].0, "A");
        assert_eq!(ranked[1].0, "B");
        assert!((ranked[0].1 - ranked[1].1).abs() < 1e-9);
    }
}
