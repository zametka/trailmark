//! Ids, file paths, and legacy alias patterns. Prefixes and the rule-key shape come from
//! `trail.toml` `[ids]`. With that table omitted, a new project mints `RULE-1`, `DEC-0001`,
//! `OPEN-0001`. A project that already numbers rules `2.N` sets `rule_style = "section"`.

use regex::Regex;
use std::cell::RefCell;
use std::collections::BTreeSet;

/// The longest id of any type: a `FACT-` id, whose key is a slug of up to 40 characters.
pub const MAX_ID_LEN: usize = 45;

/// Prefixes and the shape of a rule key. Loaded from `[ids]`; `Default` is what `trailmark init` writes.
#[derive(Clone, Debug)]
pub struct IdScheme {
    pub rule: String,
    pub charter: String,
    /// Further prefixes accepted for a charter (an imported step list, for example).
    pub charter_also: Vec<String>,
    pub decision: String,
    pub discussion: String,
    pub item: String,
    pub loop_prefix: String,
    pub review: String,
    pub open: String,
    pub deploy: String,
    pub fact: String,
    pub incident: String,
    /// `number` mints `RULE-12`. `section` mints `<rule>-2.12`.
    pub rule_style: String,
}

impl Default for IdScheme {
    fn default() -> Self {
        Self {
            rule: "RULE".into(),
            charter: "CHARTER".into(),
            charter_also: Vec::new(),
            decision: "DEC".into(),
            discussion: "DSC".into(),
            item: "ITEM".into(),
            loop_prefix: "LOOP".into(),
            review: "REV".into(),
            open: "OPEN".into(),
            deploy: "DEP".into(),
            fact: "FACT".into(),
            incident: "INC".into(),
            rule_style: "number".into(),
        }
    }
}

impl IdScheme {
    pub fn prefixes(&self) -> Vec<String> {
        let mut out = vec![
            self.rule.clone(),
            self.charter.clone(),
            self.decision.clone(),
            self.discussion.clone(),
            self.item.clone(),
            self.loop_prefix.clone(),
            self.review.clone(),
            self.open.clone(),
            self.deploy.clone(),
            self.fact.clone(),
            self.incident.clone(),
        ];
        out.extend(self.charter_also.clone());
        out.sort();
        out.dedup();
        out
    }

    pub fn prefix_for(&self, type_name: &str) -> Option<&str> {
        Some(match type_name {
            "rule" => &self.rule,
            "charter" => &self.charter,
            "decision" => &self.decision,
            "discussion" => &self.discussion,
            "item" => &self.item,
            "loop" => &self.loop_prefix,
            "review" => &self.review,
            "open" => &self.open,
            "deploy" => &self.deploy,
            "fact" => &self.fact,
            "incident" => &self.incident,
            _ => return None,
        })
    }

    pub fn section_rules(&self) -> bool {
        self.rule_style == "section"
    }
}

thread_local! {
    static SCHEME: RefCell<IdScheme> = RefCell::new(IdScheme::default());
}

pub fn scheme() -> IdScheme {
    SCHEME.with(|c| c.borrow().clone())
}

pub fn set_scheme(s: IdScheme) {
    SCHEME.with(|c| *c.borrow_mut() = s);
}

/// Restores the previous scheme when dropped, so a test can opt into another one.
#[cfg(test)]
pub struct SchemeGuard(IdScheme);

#[cfg(test)]
impl Drop for SchemeGuard {
    fn drop(&mut self) {
        set_scheme(self.0.clone());
    }
}

#[cfg(test)]
pub fn push_scheme(s: IdScheme) -> SchemeGuard {
    let old = scheme();
    set_scheme(s);
    SchemeGuard(old)
}

/// An id is at most 24 characters, except a fact id, which may be 45.
pub fn max_id_len(prefix: &str) -> usize {
    if prefix.eq_ignore_ascii_case(&scheme().fact) {
        MAX_ID_LEN
    } else {
        24
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id {
    pub prefix: String,
    pub key: String,
}

impl Id {
    pub fn canonical(&self) -> String {
        format!("{}-{}", self.prefix, self.key)
    }

    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.len() > MAX_ID_LEN || !input.is_ascii() || input.contains(' ') {
            return None;
        }
        let (pre, key) = input.split_once('-')?;
        if input.len() > max_id_len(pre) {
            return None;
        }
        if pre.is_empty() || key.is_empty() {
            return None;
        }
        if !pre.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return None;
        }
        let prefix = pre.to_ascii_uppercase();
        let scheme = scheme();
        if !scheme.prefixes().iter().any(|p| p == &prefix) {
            return None;
        }
        Some(Id {
            prefix: prefix.clone(),
            key: canon_key(&scheme, &prefix, key),
        })
    }
}

fn canon_key(scheme: &IdScheme, prefix: &str, key: &str) -> String {
    if prefix.eq_ignore_ascii_case(&scheme.fact) {
        return key.to_ascii_lowercase();
    }
    if prefix.eq_ignore_ascii_case(&scheme.rule) && scheme.section_rules() {
        let lower = key.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("2.") {
            let (num, letter) = split_num_letter(rest);
            return match letter {
                Some(c) => format!("2.{num}{c}"),
                None => format!("2.{num}"),
            };
        }
        if key.chars().all(|c| c.is_ascii_digit()) {
            return key.to_string();
        }
        return key.to_ascii_lowercase();
    }
    if prefix.eq_ignore_ascii_case(&scheme.rule) {
        return canon_number_key(key);
    }
    if prefix.eq_ignore_ascii_case(&scheme.decision)
        || prefix.eq_ignore_ascii_case(&scheme.discussion)
        || prefix.eq_ignore_ascii_case(&scheme.open)
        || prefix.eq_ignore_ascii_case(&scheme.incident)
        || prefix.eq_ignore_ascii_case(&scheme.deploy)
    {
        return key.to_string();
    }
    if prefix.eq_ignore_ascii_case(&scheme.loop_prefix)
        || prefix.eq_ignore_ascii_case(&scheme.review)
        || prefix.eq_ignore_ascii_case(&scheme.item)
    {
        return canon_code(key);
    }
    if key.chars().all(|c| c.is_ascii_digit()) {
        key.to_string()
    } else {
        key.to_ascii_lowercase()
    }
}

fn canon_number_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    let digits: String = lower.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return lower;
    }
    let rest = &lower[digits.len()..];
    if rest.is_empty() {
        return digits;
    }
    if rest.len() == 1 && rest.chars().next().unwrap().is_ascii_lowercase() {
        return format!("{digits}{rest}");
    }
    lower
}

fn split_num_letter(rest: &str) -> (&str, Option<char>) {
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() == rest.len() {
        return (rest, None);
    }
    if digits.len() + 1 == rest.len() {
        let c = rest.chars().last().unwrap();
        if c.is_ascii_alphabetic() {
            return (&rest[..digits.len()], Some(c.to_ascii_lowercase()));
        }
    }
    (rest, None)
}

fn canon_code(key: &str) -> String {
    let (head, suffix) = match key.rsplit_once('.') {
        Some((h, s))
            if !s.is_empty()
                && s.chars().all(|c| c.is_ascii_digit())
                && h.chars().any(|c| c.is_ascii_alphabetic()) =>
        {
            (h, Some(s.to_string()))
        }
        _ => (key, None),
    };
    let chars: Vec<char> = head.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_ascii_alphabetic() {
        i += 1;
    }
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if i == 0 || i > 4 || j == i {
        return if let Some(s) = suffix {
            format!("{head}.{s}")
        } else {
            head.to_string()
        };
    }
    let letters: String = chars[..i].iter().collect::<String>().to_ascii_uppercase();
    let digits: String = chars[i..j].iter().collect();
    let tail: String = chars[j..].iter().collect::<String>().to_ascii_lowercase();
    let mut out = format!("{letters}{digits}{tail}");
    if let Some(s) = suffix {
        out.push('.');
        out.push_str(&s);
    }
    out
}

#[derive(Clone, Debug)]
pub struct AliasPattern {
    pub re: Regex,
    pub id_template: Option<String>,
    pub range_template: Option<String>,
    pub context_only: bool,
}

#[derive(Clone, Debug)]
pub enum Resolved {
    One(Id),
    Range(Vec<Id>),
    Ambiguous(Vec<Id>),
    Unknown,
    Malformed,
}

pub fn apply_caps(template: &str, caps: &regex::Captures) -> String {
    let mut out = template.to_string();
    for i in (1..caps.len()).rev() {
        if let Some(m) = caps.get(i) {
            out = out.replace(&format!("${i}"), m.as_str());
        }
    }
    out
}

/// A short second number (`1235` and `6`) is the last digits of the end.
pub fn widen_number(start: &str, end: &str) -> String {
    if end.len() >= start.len() {
        return end.to_string();
    }
    let keep = start.len() - end.len();
    format!("{}{end}", &start[..keep])
}

pub fn rule_number(key: &str) -> Option<u32> {
    let rest = if scheme().section_rules() {
        key.strip_prefix("2.")?
    } else {
        key
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

pub fn folder_for(type_name: &str) -> Option<&'static str> {
    Some(match type_name {
        "rule" => "rules",
        "charter" => "charter",
        "decision" => "decisions",
        "discussion" => "discussions",
        "item" => "items",
        "loop" => "loops",
        "review" => "reviews",
        "open" => "open",
        "deploy" => "deploys",
        "fact" => "facts",
        "incident" => "incidents",
        _ => return None,
    })
}

pub fn rel_path(id: &Id, type_name: &str, local: bool) -> Option<String> {
    let folder = folder_for(type_name)?;
    let file = if type_name == "charter" {
        format!("{folder}/{}.md", id.canonical())
    } else {
        format!("{folder}/{}.md", id.key)
    };
    Some(if local { format!("local/{file}") } else { file })
}

pub fn expand_numeric_range(
    prefix: &str,
    start_key: &str,
    end_key: &str,
    existing: &BTreeSet<String>,
) -> Vec<Id> {
    let scheme = scheme();
    if prefix == scheme.rule {
        if let (Some(a), Some(b)) = (rule_number(start_key), rule_number(end_key)) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let mut out = Vec::new();
            for n in lo..=hi {
                let key = if scheme.section_rules() {
                    format!("2.{n}")
                } else {
                    format!("{n}")
                };
                let id = Id {
                    prefix: scheme.rule.clone(),
                    key,
                };
                if existing.contains(&id.canonical()) {
                    out.push(id);
                }
                // lettered siblings in the range are not implied
            }
            return out;
        }
    }
    Vec::new()
}

pub fn grammar_ok(id: &Id, type_name: &str) -> bool {
    let key = &id.key;
    let ok = match type_name {
        "rule" => rule_key_ok(key),
        "charter" => !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "decision" | "discussion" | "open" | "incident" => {
            key.len() == 4 && key.chars().all(|c| c.is_ascii_digit())
        }
        "item" | "loop" | "review" => code_key_ok(key) || (type_name == "item" && item_key_ok(key)),
        "deploy" => deploy_key_ok(key),
        "fact" => fact_key_ok(key),
        _ => false,
    };
    ok && id.canonical().len() <= max_id_len(&id.prefix)
}

fn rule_key_ok(key: &str) -> bool {
    let rest = if scheme().section_rules() {
        let Some(rest) = key.strip_prefix("2.") else {
            return false;
        };
        rest
    } else if key.contains('.') {
        return false;
    } else {
        key
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return false;
    }
    let n: u32 = digits.parse().unwrap_or(0);
    if !(1..=99999).contains(&n) {
        return false;
    }
    let left = &rest[digits.len()..];
    left.is_empty() || (left.len() == 1 && left.chars().next().unwrap().is_ascii_lowercase())
}

fn code_key_ok(key: &str) -> bool {
    let (head, suf) = match key.split_once('.') {
        Some((h, s)) => (h, Some(s)),
        None => (key, None),
    };
    if let Some(s) = suf {
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    let chars: Vec<char> = head.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_ascii_uppercase() {
        i += 1;
    }
    if i == 0 || i > 4 {
        return false;
    }
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == i {
        return false;
    }
    let tail = &chars[j..];
    tail.is_empty() || (tail.len() == 1 && tail[0].is_ascii_lowercase())
}

fn item_key_ok(key: &str) -> bool {
    let (head, suf) = match key.split_once('.') {
        Some((h, s)) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => (h, true),
        _ => (key, false),
    };
    let _ = suf;
    let rest = head.strip_prefix('P').unwrap_or("");
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn deploy_key_ok(key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    !parts.is_empty()
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit()))
}

fn fact_key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !key.starts_with('-')
        && !key.ends_with('-')
}

pub fn type_prefix_ok(type_name: &str, prefix: &str) -> bool {
    let scheme = scheme();
    if type_name == "charter" && scheme.charter_also.iter().any(|p| p == prefix) {
        return true;
    }
    scheme.prefix_for(type_name) == Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_and_code() {
        assert_eq!(Id::parse("rule-12").unwrap().canonical(), "RULE-12");
        assert!(Id::parse("guide-2.12").is_none());
        assert_eq!(Id::parse("loop-bk1").unwrap().canonical(), "LOOP-BK1");
        assert_eq!(Id::parse("LOOP-dk1d").unwrap().canonical(), "LOOP-DK1d");
        assert_eq!(Id::parse("loop-b1.2").unwrap().canonical(), "LOOP-B1.2");
        assert_eq!(
            Id::parse("fact-Oven-Log").unwrap().canonical(),
            "FACT-oven-log"
        );
    }

    #[test]
    fn section_style_is_opt_in() {
        let _guard = push_scheme(IdScheme {
            rule: "LESSON".into(),
            charter: "LESSON".into(),
            charter_also: vec!["NOTE".into()],
            rule_style: "section".into(),
            ..IdScheme::default()
        });
        assert_eq!(Id::parse("lesson-2.12").unwrap().canonical(), "LESSON-2.12");
        assert!(grammar_ok(&Id::parse("LESSON-2.4").unwrap(), "rule"));
        assert!(type_prefix_ok("charter", "NOTE"));
    }

    #[test]
    fn short_range_end() {
        assert_eq!(widen_number("1235", "6"), "1236");
        assert_eq!(widen_number("1520", "1526"), "1526");
    }
}
