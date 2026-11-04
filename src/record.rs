use crate::frontmatter::{split_record, Frontmatter};
use crate::id::Id;
use crate::util::slug;

#[derive(Clone, Debug)]
pub struct Record {
    pub rel: String,
    pub raw: Vec<u8>,
    pub fm: Frontmatter,
    pub body: String,
    pub body_line: usize,
}

impl Record {
    pub fn id(&self) -> Option<Id> {
        self.fm.get_str("id").and_then(Id::parse)
    }

    pub fn id_str(&self) -> String {
        self.fm.get_str("id").unwrap_or("").to_string()
    }

    pub fn type_name(&self) -> &str {
        self.fm.get_str("type").unwrap_or("")
    }

    pub fn title(&self) -> &str {
        self.fm.get_str("title").unwrap_or("")
    }

    pub fn status(&self) -> &str {
        self.fm.get_str("status").unwrap_or("")
    }

    pub fn legacy(&self) -> bool {
        self.fm.get_bool("legacy")
    }

    pub fn pinned(&self) -> bool {
        self.fm.get_bool("pin")
    }

    pub fn areas(&self) -> Vec<String> {
        self.fm.strings("areas")
    }

    pub fn date(&self) -> &str {
        self.fm.get_str("date").unwrap_or("")
    }

    pub fn field_line(&self, key: &str) -> usize {
        let text = String::from_utf8_lossy(&self.raw);
        for (i, line) in text.lines().enumerate() {
            if let Some((k, _)) = line.split_once(':') {
                if k.trim() == key {
                    return i + 1;
                }
            }
            if line.trim() == "---" && i > 0 {
                break;
            }
        }
        1
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub text: String,
    pub line: usize,
}

pub fn parts_of(rec: &Record) -> Vec<Part> {
    let mut out = Vec::new();
    let lines: Vec<&str> = rec.body.lines().collect();
    let mut rule_n = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let file_line = rec.body_line + i;
        if line.trim_start().starts_with("**Incident") {
            let start = i;
            i += 1;
            while i < lines.len() {
                let n = lines[i];
                if n.trim().is_empty() || n.starts_with("> **Rule.") || n.starts_with('#') {
                    break;
                }
                i += 1;
            }
            out.push(Part {
                name: "incident".into(),
                text: join(&lines[start..i]),
                line: file_line,
            });
            continue;
        }
        if line.starts_with("> **Rule.**") || line.starts_with("> **Rule.**") {
            let start = i;
            i += 1;
            while i < lines.len() && lines[i].starts_with('>') {
                i += 1;
            }
            rule_n += 1;
            let name = if rule_n == 1 {
                "rule".into()
            } else {
                format!("rule-{rule_n}")
            };
            out.push(Part {
                name,
                text: join(&lines[start..i]),
                line: file_line,
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            let name = slug(rest);
            let start = i;
            i += 1;
            while i < lines.len() && !lines[i].starts_with("## ") && !lines[i].starts_with("### ") {
                i += 1;
            }
            if !name.is_empty() {
                out.push(Part {
                    name,
                    text: join(&lines[start..i]),
                    line: file_line,
                });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            let token = rest.split_whitespace().next().unwrap_or("");
            let token = token.trim_end_matches(['—', '-', ':']);
            // Legacy rule headings (### 2.4 …) are not parts.
            if !token.is_empty()
                && !token.contains('.')
                && token.chars().all(|c| c.is_ascii_alphanumeric())
            {
                let start = i;
                i += 1;
                while i < lines.len()
                    && !lines[i].starts_with("### ")
                    && !lines[i].starts_with("## ")
                {
                    i += 1;
                }
                out.push(Part {
                    name: token.to_string(),
                    text: join(&lines[start..i]),
                    line: file_line,
                });
                continue;
            }
        }
        if let Some(code) = remark_code(line) {
            let start = i;
            i += 1;
            while i < lines.len() && remark_code(lines[i]).is_none() && !lines[i].starts_with('#') {
                i += 1;
            }
            out.push(Part {
                name: code,
                text: join(&lines[start..i]),
                line: file_line,
            });
            continue;
        }
        i += 1;
    }
    out
}

fn join(lines: &[&str]) -> String {
    let mut s = lines.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// A remark's code at the start of a line: `**A1.** …`, `- **A1. …`, `- **B2. …`,
/// `**K9e — …`. One to three capitals, digits, an optional small letter, then a stop.
fn remark_code(line: &str) -> Option<String> {
    let t = line.trim_start();
    let t = t.strip_prefix("- ").unwrap_or(t);
    let rest = t.strip_prefix("**")?;
    let caps = rest.chars().take_while(|c| c.is_ascii_uppercase()).count();
    if caps == 0 || caps > 3 {
        return None;
    }
    let digits = rest[caps..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let mut end = caps + digits;
    if let Some(c) = rest[end..].chars().next() {
        if c.is_ascii_lowercase() {
            end += 1;
        }
    }
    let after = &rest[end..];
    let stops = after.starts_with('.')
        || after.starts_with(':')
        || after.starts_with("**")
        || after.starts_with(' ');
    if stops {
        Some(rest[..end].to_string())
    } else {
        None
    }
}

pub fn find_part<'a>(parts: &'a [Part], name: &str) -> Option<&'a Part> {
    let want = name.trim();
    let want_slug = slug(want);
    parts
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(want) || p.name == want_slug)
}

pub fn load_record(rel: &str, raw: Vec<u8>) -> Result<Record, (usize, String)> {
    let text = String::from_utf8(raw.clone()).map_err(|_| (1, "file is not UTF-8".to_string()))?;
    match split_record(&text) {
        Ok((fm, body, body_line)) => Ok(Record {
            rel: rel.to_string(),
            raw,
            fm,
            body,
            body_line,
        }),
        Err(e) => Err((e.line, e.message)),
    }
}

pub const KNOWN_KEYS: &[&str] = &[
    "id",
    "type",
    "title",
    "date",
    "status",
    "areas",
    "short",
    "source",
    "links",
    "supersedes",
    "superseded_by",
    "narrows",
    "narrowed_by",
    "either",
    "follows",
    "aliases",
    "pin",
    "legacy",
    "updated",
    "retired_note",
    "status_note",
    "status_history",
    "date_note",
    "areas_note",
    "earned_by",
    "enforced_in",
    "stale_note",
    "decided_by",
    "basis",
    "quote",
    "decided_in",
    "scope",
    "from",
    "channel",
    "items",
    "decisions",
    "asked_by",
    "order",
    "platform",
    "loops",
    "deploy",
    "waiting_on",
    "item",
    "started",
    "closed",
    "worktree",
    "branch",
    "commits",
    "gates",
    "learned",
    "opened",
    "closes",
    "review",
    "migration",
    "reviews",
    "commits_read",
    "verdict",
    "for_deploy",
    "kind",
    "severity",
    "owner",
    "opened_by",
    "closed_by",
    "target",
    "time",
    "authorized_by",
    "word",
    "word_date",
    "backup",
    "backup_word",
    "commit",
    "binary_sha256",
    "migrations",
    "new_migrations",
    "carries",
    "previous",
    "rollback",
    "release_notes",
    "owed",
    "version",
    "version_code",
    "apk_sha256",
    "devices",
    "applies_to",
    "missed",
    "production",
    "found_by",
    "earned",
    "deploys",
    "figures",
    "migrated_from",
];

/// A `figures` entry is `<path under the store> | <caption>`.
pub fn split_figure(raw: &str) -> (String, String) {
    match raw.split_once('|') {
        Some((p, c)) => (p.trim().to_string(), c.trim().to_string()),
        None => (raw.trim().to_string(), String::new()),
    }
}

pub fn is_tombstone(status: &str) -> bool {
    status == "retired" || status == "superseded"
}

/// Values that share a link field with ids but are not ids.
pub fn is_sentinel(s: &str) -> bool {
    matches!(
        s,
        "none" | "unknown" | "owed" | "author" | "orchestrator" | "next-loop"
    )
}
