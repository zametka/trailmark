use crate::id::{AliasPattern, IdScheme};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Area {
    pub keywords: Vec<String>,
    /// Accepted on an area so a project can name the paths it covers. The scanner reads `[code]`.
    #[allow(dead_code)]
    pub paths: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Budgets {
    pub core: usize,
    pub context_default: usize,
    pub context_max: usize,
    /// `trailmark terms`: the whole term index an agent reads before `--terms`.
    pub terms: usize,
    pub rules_area_index: usize,
    pub hot_rules: usize,
    pub open_index: usize,
    pub decisions_index: usize,
    pub loops_index: usize,
    pub deploys_index: usize,
    pub items_index: usize,
    /// `index/start.md`: the pointer a harness's own memory index can be replaced with.
    pub start_index: usize,
    pub record_warn: BTreeMap<String, usize>,
    /// Lines of most-cited rules in the core. The rest is in `index/rules-hot.md`.
    pub core_hot_rules: usize,
    /// Fold an area index that cannot list every rule (top-cited lines, then a count)
    /// instead of failing. Off by default, as the spec says; a host turns it on in
    /// `trail.toml` and `trailmark index` names every folded file.
    pub fold_rules_area: bool,
}

impl Default for Budgets {
    fn default() -> Self {
        let mut record_warn = BTreeMap::new();
        record_warn.insert("rule".into(), 2000);
        record_warn.insert("loop".into(), 10000);
        record_warn.insert("review".into(), 10000);
        Self {
            core: 2500,
            context_default: 8000,
            context_max: 20000,
            terms: 3200,
            rules_area_index: 3000,
            hot_rules: 1200,
            open_index: 2500,
            decisions_index: 3000,
            loops_index: 1500,
            deploys_index: 800,
            items_index: 4000,
            start_index: 400,
            record_warn,
            core_hot_rules: 15,
            fold_rules_area: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MeasureTask {
    pub name: String,
    pub text: String,
    pub areas: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub requires: String,
    pub store_rel: String,
    pub name: Option<String>,
    pub frozen: bool,
    pub cutover: bool,
    /// `commit_index = true`: `index/` is committed and `check` refuses a stale file (C08).
    /// The default (false): `index/` is a local cache with its own `.gitignore`. Every header
    /// carried the store hash, so two worktrees that each added a record conflicted on every
    /// index file when they landed.
    pub commit_index: bool,
    pub budgets: Budgets,
    pub reserve_rule: u32,
    pub reserve_open: u32,
    pub areas: BTreeMap<String, Area>,
    pub code_include: Vec<String>,
    pub code_exclude: Vec<String>,
    pub alias_patterns: Vec<AliasPattern>,
    pub never_whole: Vec<String>,
    /// `[adapters.<table>] memory_md`, by table. An adapter's `install.toml` names its table.
    pub adapter_memory: BTreeMap<String, String>,
    /// `skip`: in a legacy body, a `context_only` alias form (`#85`) that resolves to no
    /// record is not reported. Old prose uses `#N` for deploys, reviews and builds alike,
    /// and migrated text cannot be edited. `error` (the default) reports it as C05.
    pub legacy_context_refs: String,
    pub measure_tasks: Vec<MeasureTask>,
    pub ids: IdScheme,
    /// Charter id of the review step included in a `--review` bundle. Follows `superseded_by`.
    pub review_step: Option<String>,
}

impl Config {
    pub fn store_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.store_rel)
    }
}

/// The machine-local overlay: tables in `.trail/local.toml` replace the same keys of
/// `trail.toml`. It holds what differs per machine (a home directory, a harness's memory
/// folder) so the committed file names no person's paths. `.trail/` is gitignored.
pub const LOCAL_OVERLAY: &str = ".trail/local.toml";

pub fn load_config(root: &Path) -> Result<Config, String> {
    let path = root.join("trail.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| format!("no trail.toml in {}", root.display()))?;
    let mut v: toml::Value = toml::from_str(&text).map_err(|e| format!("trail.toml: {e}"))?;
    check_keys(&v, "trail.toml")?;
    let local = root.join(LOCAL_OVERLAY);
    if let Ok(more) = std::fs::read_to_string(&local) {
        let over: toml::Value =
            toml::from_str(&more).map_err(|e| format!("{LOCAL_OVERLAY}: {e}"))?;
        check_keys(&over, LOCAL_OVERLAY)?;
        merge(&mut v, over);
    }
    from_value(&v)
}

fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot @ toml::Value::Table(_)) if v.is_table() => merge(slot, v),
                    _ => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

const TOP_KEYS: &[&str] = &[
    "name",
    "requires",
    "store",
    "cutover",
    "frozen",
    "commit_index",
    "never_whole",
    "budgets",
    "reserve",
    "areas",
    "code",
    "alias_pattern",
    "measure",
    "review_step",
    "adapters",
    "check",
    "ids",
];
const BUDGET_KEYS: &[&str] = &[
    "core",
    "context_default",
    "context_max",
    "terms",
    "rules_area_index",
    "hot_rules",
    "open_index",
    "decisions_index",
    "loops_index",
    "deploys_index",
    "items_index",
    "record_warn",
    "core_hot_rules",
    "fold_rules_area",
    "start_index",
];

/// A key this binary does not know is an error, with the nearest known key. A setting that
/// is silently ignored is how a deploy almost shipped an open origin.
pub fn check_keys(v: &toml::Value, file: &str) -> Result<(), String> {
    let mut bad = Vec::new();
    let mut want = |where_: &str, table: Option<&toml::Value>, allowed: &[&str]| {
        if let Some(t) = table.and_then(|t| t.as_table()) {
            for key in t.keys() {
                if !allowed.contains(&key.as_str()) {
                    let hint = crate::util::suggest(key, allowed.iter().copied())
                        .map(|s| format!(" (did you mean '{s}'?)"))
                        .unwrap_or_default();
                    bad.push(format!("{file}: unknown key '{where_}{key}'{hint}"));
                }
            }
        }
    };
    want("", Some(v), TOP_KEYS);
    want("budgets.", v.get("budgets"), BUDGET_KEYS);
    want("reserve.", v.get("reserve"), &["rule", "open"]);
    want("code.", v.get("code"), &["include", "exclude"]);
    want("measure.", v.get("measure"), &["task"]);
    want("check.", v.get("check"), &["legacy_context_refs"]);
    want(
        "ids.",
        v.get("ids"),
        &[
            "rule",
            "charter",
            "charter_also",
            "decision",
            "discussion",
            "item",
            "loop",
            "review",
            "open",
            "deploy",
            "fact",
            "incident",
            "rule_style",
        ],
    );
    if let Some(areas) = v.get("areas").and_then(|a| a.as_table()) {
        for (name, spec) in areas {
            want(
                &format!("areas.{name}."),
                Some(spec),
                &["keywords", "paths"],
            );
        }
    }
    if let Some(adapters) = v.get("adapters").and_then(|a| a.as_table()) {
        for (name, spec) in adapters {
            want(&format!("adapters.{name}."), Some(spec), &["memory_md"]);
        }
    }
    for (i, item) in v
        .get("alias_pattern")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        want(
            &format!("alias_pattern[{i}]."),
            Some(item),
            &["match", "id", "range", "context_only"],
        );
    }
    for (i, item) in v
        .get("measure")
        .and_then(|m| m.get("task"))
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        want(
            &format!("measure.task[{i}]."),
            Some(item),
            &["name", "text", "areas"],
        );
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("\n"))
    }
}

/// `~/` at the start of a configured path is the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn from_value(v: &toml::Value) -> Result<Config, String> {
    let requires = v
        .get("requires")
        .and_then(|x| x.as_str())
        .unwrap_or(">=0.1.0")
        .to_string();
    let store_rel = v
        .get("store")
        .and_then(|x| x.as_str())
        .ok_or("trail.toml: missing store")?
        .to_string();
    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let frozen = v.get("frozen").and_then(|x| x.as_bool()).unwrap_or(false);
    let cutover = v.get("cutover").and_then(|x| x.as_bool()).unwrap_or(false);
    let commit_index = v
        .get("commit_index")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut budgets = Budgets::default();
    if let Some(b) = v.get("budgets") {
        budgets.core = int_or(b, "core", budgets.core);
        budgets.context_default = int_or(b, "context_default", budgets.context_default);
        budgets.context_max = int_or(b, "context_max", budgets.context_max);
        budgets.terms = int_or(b, "terms", budgets.terms);
        budgets.rules_area_index = int_or(b, "rules_area_index", budgets.rules_area_index);
        budgets.hot_rules = int_or(b, "hot_rules", budgets.hot_rules);
        budgets.open_index = int_or(b, "open_index", budgets.open_index);
        budgets.decisions_index = int_or(b, "decisions_index", budgets.decisions_index);
        budgets.loops_index = int_or(b, "loops_index", budgets.loops_index);
        budgets.deploys_index = int_or(b, "deploys_index", budgets.deploys_index);
        budgets.items_index = int_or(b, "items_index", budgets.items_index);
        budgets.start_index = int_or(b, "start_index", budgets.start_index);
        budgets.core_hot_rules = int_or(b, "core_hot_rules", budgets.core_hot_rules);
        budgets.fold_rules_area = b
            .get("fold_rules_area")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        if let Some(w) = b.get("record_warn").and_then(|x| x.as_table()) {
            for (k, val) in w {
                if let Some(n) = val.as_integer() {
                    budgets.record_warn.insert(k.clone(), n as usize);
                }
            }
        }
    }
    let reserve_rule = v
        .get("reserve")
        .and_then(|r| r.get("rule"))
        .and_then(|x| x.as_integer())
        .unwrap_or(10) as u32;
    let reserve_open = v
        .get("reserve")
        .and_then(|r| r.get("open"))
        .and_then(|x| x.as_integer())
        .unwrap_or(10) as u32;

    let mut areas = BTreeMap::new();
    if let Some(table) = v.get("areas").and_then(|x| x.as_table()) {
        for (name, spec) in table {
            let keywords = string_list(spec.get("keywords"));
            let paths = string_list(spec.get("paths"));
            areas.insert(name.clone(), Area { keywords, paths });
        }
    }

    let code_include = string_list(v.get("code").and_then(|c| c.get("include")));
    let code_exclude = string_list(v.get("code").and_then(|c| c.get("exclude")));
    let never_whole = string_list(v.get("never_whole"));

    let mut alias_patterns = Vec::new();
    if let Some(arr) = v.get("alias_pattern").and_then(|x| x.as_array()) {
        for item in arr {
            let pat = item
                .get("match")
                .and_then(|x| x.as_str())
                .ok_or("alias_pattern missing match")?;
            let re = Regex::new(pat).map_err(|e| format!("alias pattern '{pat}': {e}"))?;
            alias_patterns.push(AliasPattern {
                re,
                id_template: item
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                range_template: item
                    .get("range")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                context_only: item
                    .get("context_only")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            });
        }
    }

    // Any adapter table may carry memory_md. The core does not name a harness.
    let mut adapter_memory = BTreeMap::new();
    if let Some(table) = v.get("adapters").and_then(|a| a.as_table()) {
        for (name, entry) in table {
            if let Some(path) = entry.get("memory_md").and_then(|x| x.as_str()) {
                adapter_memory.insert(name.clone(), path.to_string());
            }
        }
    }

    let legacy_context_refs = v
        .get("check")
        .and_then(|c| c.get("legacy_context_refs"))
        .and_then(|x| x.as_str())
        .unwrap_or("error")
        .to_string();
    if !matches!(legacy_context_refs.as_str(), "error" | "skip") {
        return Err(format!(
            "trail.toml: check.legacy_context_refs is '{legacy_context_refs}'; use error or skip"
        ));
    }

    let review_step = v
        .get("review_step")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());

    let mut measure_tasks = Vec::new();
    if let Some(arr) = v
        .get("measure")
        .and_then(|m| m.get("task"))
        .and_then(|x| x.as_array())
    {
        for item in arr {
            measure_tasks.push(MeasureTask {
                name: item
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("task")
                    .to_string(),
                text: item
                    .get("text")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                areas: string_list(item.get("areas")),
            });
        }
    }

    let ids = ids_from(v.get("ids"))?;
    crate::id::set_scheme(ids.clone());

    Ok(Config {
        requires,
        store_rel,
        name,
        frozen,
        cutover,
        commit_index,
        budgets,
        reserve_rule,
        reserve_open,
        areas,
        code_include,
        code_exclude,
        alias_patterns,
        never_whole,
        adapter_memory,
        legacy_context_refs,
        measure_tasks,
        ids,
        review_step,
    })
}

fn ids_from(v: Option<&toml::Value>) -> Result<IdScheme, String> {
    let mut ids = IdScheme::default();
    let Some(v) = v else {
        return Ok(ids);
    };
    let take = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .map(|s| s.to_ascii_uppercase())
    };
    if let Some(x) = take("rule") {
        ids.rule = x;
    }
    if let Some(x) = take("charter") {
        ids.charter = x;
    }
    if let Some(x) = take("decision") {
        ids.decision = x;
    }
    if let Some(x) = take("discussion") {
        ids.discussion = x;
    }
    if let Some(x) = take("item") {
        ids.item = x;
    }
    if let Some(x) = take("loop") {
        ids.loop_prefix = x;
    }
    if let Some(x) = take("review") {
        ids.review = x;
    }
    if let Some(x) = take("open") {
        ids.open = x;
    }
    if let Some(x) = take("deploy") {
        ids.deploy = x;
    }
    if let Some(x) = take("fact") {
        ids.fact = x;
    }
    if let Some(x) = take("incident") {
        ids.incident = x;
    }
    if let Some(x) = v.get("rule_style").and_then(|s| s.as_str()) {
        if x != "number" && x != "section" {
            return Err(format!(
                "trail.toml: ids.rule_style is '{x}'; use number or section"
            ));
        }
        ids.rule_style = x.to_string();
    }
    if let Some(arr) = v.get("charter_also").and_then(|x| x.as_array()) {
        ids.charter_also = arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_ascii_uppercase()))
            .collect();
    }
    for p in ids.prefixes() {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!("trail.toml: id prefix '{p}' must be letters"));
        }
    }
    Ok(ids)
}

fn int_or(v: &toml::Value, key: &str, default: usize) -> usize {
    v.get(key)
        .and_then(|x| x.as_integer())
        .map(|n| n as usize)
        .unwrap_or(default)
}

fn string_list(v: Option<&toml::Value>) -> Vec<String> {
    match v.and_then(|x| x.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
        None => Vec::new(),
    }
}

pub fn version_ok(requires: &str, current: &str) -> Result<(), String> {
    let req = requires.trim();
    let (op, raw) = if let Some(rest) = req.strip_prefix(">=") {
        (">=", rest.trim())
    } else {
        ("=", req)
    };
    let want = parse_ver(raw).ok_or_else(|| format!("bad requires '{requires}'"))?;
    let have = parse_ver(current).ok_or_else(|| format!("bad version '{current}'"))?;
    let ok = if op == ">=" {
        have >= want
    } else {
        have == want
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "this trailmark is {current}; trail.toml requires {requires}"
        ))
    }
}

fn parse_ver(s: &str) -> Option<(u64, u64, u64)> {
    let mut p = s.split('.');
    Some((
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
    ))
}

pub fn find_root(start: &Path) -> Result<PathBuf, String> {
    let mut cur = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if cur.join("trail.toml").is_file() {
            return Ok(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    Err("no trail.toml found walking up from the working directory; pass --root".into())
}
