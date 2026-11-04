use crate::config::{self, Config};
use crate::id::{self, apply_caps, widen_number, Id, Resolved};
use crate::issue::Problem;
use crate::record::{self, Record};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Reservation {
    pub type_name: String,
    pub from: u32,
    pub to: u32,
    pub for_id: String,
    pub date: String,
    /// The checkout that took the block; written in the shared ledger only.
    pub worktree: String,
}

#[derive(Clone, Debug)]
pub struct Ambiguous {
    pub written: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Waiver {
    pub code: String,
    pub path: String,
    pub line: usize,
    pub written: String,
    pub by: String,
    pub date: String,
    pub why: String,
}

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
    pub config: Config,
    pub records: Vec<Record>,
    pub broken: Vec<Problem>,
    pub reservations: Vec<Reservation>,
    pub ambiguous: Vec<Ambiguous>,
    pub waivers: Vec<Waiver>,
    /// The blocks every worktree of this repository has taken (`ledger.rs`); `None` outside
    /// a git repository and in a dry run.
    pub ledger: Option<crate::ledger::Ledger>,
    cache: Cache,
}

/// Lookups built once per store and rebuilt after `invalidate` (every mutation calls it) or
/// when the number of records changes. Without them every `get` rebuilt the id map, which
/// made `context` and `check` quadratic in the number of records.
type Sized<T> = RefCell<Option<(usize, Rc<T>)>>;

#[derive(Clone, Debug, Default)]
struct Cache {
    ids: Sized<BTreeMap<String, usize>>,
    aliases: Sized<BTreeMap<String, String>>,
    code: RefCell<Option<Rc<Vec<crate::scan::Hit>>>>,
}

impl Store {
    pub fn load(root: &Path) -> Result<Self, String> {
        let config = config::load_config(root)?;
        let version = env!("CARGO_PKG_VERSION");
        config::version_ok(&config.requires, version)?;
        let dir = config.store_dir(root);
        let mut store = Self::load_dir(root, &dir, config)?;
        store.ledger = crate::ledger::load(root, &store.config.store_rel)?;
        Ok(store)
    }

    /// Every block this checkout knows and every block the shared ledger holds.
    pub fn all_reservations(&self) -> impl Iterator<Item = &Reservation> {
        self.reservations
            .iter()
            .chain(self.ledger.iter().flat_map(|l| l.entries.iter()))
    }

    /// Load records from `dir`, which may differ from `config.store_dir` during a dry run.
    pub fn load_dir(root: &Path, dir: &Path, config: Config) -> Result<Self, String> {
        crate::id::set_scheme(config.ids.clone());
        if !dir.is_dir() {
            return Err(format!(
                "store not found: {} (create this directory; it may be empty. `trailmark init` writes trail.toml and the store)",
                dir.strip_prefix(root).unwrap_or(dir).display()
            ));
        }
        let mut records = Vec::new();
        let mut broken = Vec::new();
        walk_md(dir, dir, &mut |rel, bytes| {
            if rel.starts_with("index/")
                || rel.starts_with("_migration/")
                || rel == "reservations.toml"
            {
                return;
            }
            match record::load_record(&rel, bytes) {
                Ok(rec) => records.push(rec),
                Err((line, message)) => broken.push(Problem::error("C01", rel, line, message)),
            }
        });
        records.sort_by(|a, b| a.rel.cmp(&b.rel));
        // A side file that does not parse is an error, never "nothing in it": a lost
        // reservation lets the next block start inside a running loop's numbers.
        let reservations = read_all_reservations(dir)?;
        let ambiguous = read_ambiguous(&dir.join("_migration/ambiguous.toml"))?;
        let waivers = read_waivers(&dir.join("_migration/waivers.toml"))?;
        Ok(Store {
            root: root.to_path_buf(),
            config,
            records,
            broken,
            reservations,
            ambiguous,
            waivers,
            ledger: None,
            cache: Cache::default(),
        })
    }

    /// Drop the cached lookups. Call after changing `records`.
    pub fn invalidate(&self) {
        *self.cache.ids.borrow_mut() = None;
        *self.cache.aliases.borrow_mut() = None;
        *self.cache.code.borrow_mut() = None;
    }

    pub fn id_index(&self) -> Rc<BTreeMap<String, usize>> {
        let n = self.records.len();
        if let Some((len, map)) = self.cache.ids.borrow().as_ref() {
            if *len == n {
                return map.clone();
            }
        }
        let mut map = BTreeMap::new();
        for (i, rec) in self.records.iter().enumerate() {
            if let Some(id) = rec.id() {
                map.entry(id.canonical()).or_insert(i);
            }
        }
        let map = Rc::new(map);
        *self.cache.ids.borrow_mut() = Some((n, map.clone()));
        map
    }

    pub fn get(&self, id: &str) -> Option<&Record> {
        let want = Id::parse(id)
            .map(|i| i.canonical())
            .unwrap_or_else(|| id.to_string());
        let map = self.id_index();
        let i = *map.get(&want)?;
        let rec = self.records.get(i)?;
        if rec.id().map(|x| x.canonical()) == Some(want.clone()) {
            return Some(rec);
        }
        // The records moved without an invalidate; rebuild once.
        self.invalidate();
        let i = *self.id_index().get(&want)?;
        self.records.get(i)
    }

    /// Code citations, scanned once per store.
    pub fn code_hits(&self) -> Rc<Vec<crate::scan::Hit>> {
        if let Some(hits) = self.cache.code.borrow().as_ref() {
            return hits.clone();
        }
        let mut hits = Vec::new();
        for path in crate::scan::code_files(self) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            hits.extend(crate::scan::scan_file(self, &path, &text, false, false));
        }
        let hits = Rc::new(hits);
        *self.cache.code.borrow_mut() = Some(hits.clone());
        hits
    }

    pub fn existing_ids(&self) -> BTreeSet<String> {
        self.records
            .iter()
            .filter_map(|r| r.id().map(|i| i.canonical()))
            .collect()
    }

    pub fn alias_index(&self) -> Rc<BTreeMap<String, String>> {
        let n = self.records.len();
        if let Some((len, map)) = self.cache.aliases.borrow().as_ref() {
            if *len == n {
                return map.clone();
            }
        }
        let mut map = BTreeMap::new();
        for rec in &self.records {
            let Some(id) = rec.id() else { continue };
            for alias in rec.fm.strings("aliases") {
                map.insert(compact(&alias), id.canonical());
            }
        }
        let map = Rc::new(map);
        *self.cache.aliases.borrow_mut() = Some((n, map.clone()));
        map
    }

    pub fn resolve_token(&self, raw: &str, allow_context_only: bool) -> Resolved {
        let raw = raw.trim();
        let token = id_part(raw);
        if token.is_empty() {
            return Resolved::Malformed;
        }
        if let Some(ids) = self.ambiguous_ids(token) {
            if Id::parse(token).is_none() {
                return Resolved::Ambiguous(ids);
            }
        }
        // A range first: `-2.1526` is short enough to pass the id grammar as one
        // id with a dotted key, and would then resolve to nothing.
        if token.contains("..") {
            return self.range_from(token);
        }
        if let Some(id) = Id::parse(token) {
            return Resolved::One(id);
        }
        if let Some(id) = self
            .alias_index()
            .get(&compact(token))
            .and_then(|s| Id::parse(s))
        {
            return Resolved::One(id);
        }
        for pat in &self.config.alias_patterns {
            if pat.context_only && !allow_context_only {
                continue;
            }
            let Some(caps) = pat.re.captures(token) else {
                continue;
            };
            let Some(whole) = caps.get(0) else { continue };
            if whole.start() != 0 || whole.end() != token.len() {
                continue;
            }
            if let Some(tmpl) = &pat.range_template {
                return self.range_from(&apply_caps(tmpl, &caps));
            }
            if let Some(tmpl) = &pat.id_template {
                if let Some(id) = Id::parse(&apply_caps(tmpl, &caps)) {
                    return Resolved::One(id);
                }
            }
        }
        if token.starts_with('§') || token.starts_with('#') {
            Resolved::Unknown
        } else {
            Resolved::Malformed
        }
    }

    fn range_from(&self, rendered: &str) -> Resolved {
        let Some((a, b)) = rendered.split_once("..") else {
            return Resolved::Malformed;
        };
        let Some(mut start) = Id::parse(a.trim()) else {
            return Resolved::Malformed;
        };
        let Some(mut end) = Id::parse(b.trim()) else {
            return Resolved::Malformed;
        };
        if let (Some(sn), Some(en)) = (id::rule_number(&start.key), id::rule_number(&end.key)) {
            let ss = sn.to_string();
            let es = en.to_string();
            if es.len() < ss.len() {
                let wide = widen_number(&ss, &es);
                let key = if id::scheme().section_rules() {
                    format!("2.{wide}")
                } else {
                    wide
                };
                end = Id {
                    prefix: end.prefix,
                    key,
                };
            }
            if id::rule_number(&start.key).unwrap_or(0) > id::rule_number(&end.key).unwrap_or(0) {
                std::mem::swap(&mut start, &mut end);
            }
        }
        let existing = self.existing_ids();
        let ids = id::expand_numeric_range(&start.prefix, &start.key, &end.key, &existing);
        Resolved::Range(ids)
    }

    fn ambiguous_ids(&self, token: &str) -> Option<Vec<Id>> {
        let c = compact(token);
        for group in &self.ambiguous {
            if compact(&group.written) == c {
                let ids: Vec<Id> = group.ids.iter().filter_map(|s| Id::parse(s)).collect();
                if !ids.is_empty() {
                    return Some(ids);
                }
            }
        }
        None
    }

    pub fn nearest(&self, id: &Id) -> Vec<String> {
        let mut scored: Vec<(usize, String)> = self
            .records
            .iter()
            .filter_map(|r| r.id())
            .filter(|other| other.prefix == id.prefix)
            .map(|other| {
                let d = match (id::rule_number(&id.key), id::rule_number(&other.key)) {
                    (Some(a), Some(b)) => a.abs_diff(b) as usize,
                    _ => crate::util::edit_distance(&id.key, &other.key),
                };
                (d, other.canonical())
            })
            .collect();
        scored.sort();
        scored.into_iter().take(3).map(|(_, s)| s).collect()
    }

    pub fn waived(&self, code: &str, path: &str, line: usize, written: &str) -> bool {
        self.waived_hit(code, path, line, written, &[line])
    }

    /// Is this finding waived? `same` holds the lines of every finding with this code, path
    /// and cited text (this one included). A waiver that names the cited text matches by path
    /// and text; its line is a hint: the citation on that line, or else the one nearest to it.
    /// A line pin broke `trailmark check` each time code above a waived citation moved the
    /// citation to another line.
    /// A waiver without the text still needs its line.
    pub fn waived_hit(
        &self,
        code: &str,
        path: &str,
        line: usize,
        written: &str,
        same: &[usize],
    ) -> bool {
        self.waivers.iter().any(|w| {
            if w.by.is_empty()
                || w.date.is_empty()
                || w.code != code
                || !(w.path.is_empty() || w.path == path)
                || !(w.written.is_empty() || compact(&w.written) == compact(written))
            {
                return false;
            }
            if w.line == 0 || w.line == line {
                return true;
            }
            if w.written.is_empty() || w.path.is_empty() || same.contains(&w.line) {
                return false;
            }
            let nearest = same
                .iter()
                .copied()
                .min_by_key(|l| (l.abs_diff(w.line), *l));
            nearest == Some(line)
        })
    }
}

/// The id before a `#part`. A `#` that opens the token or follows a space belongs to a
/// legacy form such as `#85` or `deploy #85`, so the whole token is kept.
pub fn id_part(raw: &str) -> &str {
    match raw.split_once('#') {
        Some((a, _)) if !a.is_empty() && !a.ends_with(char::is_whitespace) => a,
        _ => raw,
    }
}

pub fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn walk_md(dir: &Path, root: &Path, out: &mut impl FnMut(String, Vec<u8>)) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "index" || name == "_migration" {
            continue;
        }
        if path.is_dir() {
            walk_md(&path, root, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(bytes) = fs::read(&path) {
            out(rel, bytes);
        }
    }
}

fn read_side(path: &Path, table: &str) -> Result<Vec<toml::Value>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    match v.get(table) {
        None => Ok(Vec::new()),
        Some(x) => x
            .as_array()
            .cloned()
            .ok_or_else(|| format!("{}: [[{table}]] must be an array of tables", path.display())),
    }
}

fn field<'a>(
    path: &Path,
    n: usize,
    item: &'a toml::Value,
    key: &str,
) -> Result<&'a toml::Value, String> {
    item.get(key)
        .ok_or_else(|| format!("{}: entry {} has no {key}", path.display(), n + 1))
}

fn text_of(path: &Path, n: usize, item: &toml::Value, key: &str) -> Result<String, String> {
    field(path, n, item, key)?
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("{}: entry {} {key} is not a string", path.display(), n + 1))
}

fn opt_text(item: &toml::Value, key: &str) -> String {
    match item.get(key) {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Datetime(d)) => d.to_string(),
        _ => String::new(),
    }
}

/// The folder of per-owner reservation files: `reservations/LOOP-B1.toml` holds the blocks
/// taken for LOOP-B1, and only the worktree that runs that loop writes it. One shared file
/// that every `add loop` appended to conflicted at its end whenever two loops landed, and an
/// uncommitted append to it in the main checkout blocked every fast-forward.
pub const RESERVATION_DIR: &str = "reservations";

/// The store-relative path of the file that holds `for_id`'s blocks.
pub fn reservation_rel(for_id: &str) -> String {
    format!("{RESERVATION_DIR}/{for_id}.toml")
}

/// Every block a store directory holds: the legacy `reservations.toml` (read, never written
/// since ), then each `reservations/<owner>.toml` in name order.
pub(crate) fn read_all_reservations(dir: &Path) -> Result<Vec<Reservation>, String> {
    let mut out = read_reservations(&dir.join("reservations.toml"))?;
    let folder = dir.join(RESERVATION_DIR);
    if let Ok(entries) = std::fs::read_dir(&folder) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .collect();
        files.sort();
        for file in files {
            out.extend(read_reservations(&file)?);
        }
    }
    Ok(out)
}

pub(crate) fn read_reservations(path: &Path) -> Result<Vec<Reservation>, String> {
    let mut out = Vec::new();
    for (n, item) in read_side(path, "reservation")?.iter().enumerate() {
        let num = |key: &str| -> Result<u32, String> {
            field(path, n, item, key)?
                .as_integer()
                .filter(|v| *v >= 0)
                .map(|v| v as u32)
                .ok_or_else(|| format!("{}: entry {} {key} is not a number", path.display(), n + 1))
        };
        let r = Reservation {
            type_name: text_of(path, n, item, "type")?,
            from: num("from")?,
            to: num("to")?,
            for_id: text_of(path, n, item, "for")?,
            date: opt_text(item, "date"),
            worktree: opt_text(item, "worktree"),
        };
        if r.from > r.to {
            return Err(format!(
                "{}: entry {} runs backwards",
                path.display(),
                n + 1
            ));
        }
        out.push(r);
    }
    Ok(out)
}

fn read_ambiguous(path: &Path) -> Result<Vec<Ambiguous>, String> {
    let mut out = Vec::new();
    for (n, item) in read_side(path, "ambiguous")?.iter().enumerate() {
        let ids = field(path, n, item, "ids")?
            .as_array()
            .ok_or_else(|| format!("{}: entry {} ids is not a list", path.display(), n + 1))?
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
        out.push(Ambiguous {
            written: text_of(path, n, item, "written")?,
            ids,
        });
    }
    Ok(out)
}

fn read_waivers(path: &Path) -> Result<Vec<Waiver>, String> {
    let mut out = Vec::new();
    for (n, item) in read_side(path, "waiver")?.iter().enumerate() {
        out.push(Waiver {
            code: text_of(path, n, item, "code")?,
            path: opt_text(item, "path"),
            line: item.get("line").and_then(|x| x.as_integer()).unwrap_or(0) as usize,
            written: opt_text(item, "written"),
            by: text_of(path, n, item, "by")?,
            date: opt_text(item, "date"),
            why: text_of(path, n, item, "why")?,
        });
    }
    Ok(out)
}
