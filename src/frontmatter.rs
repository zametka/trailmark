//! The restricted YAML from the spec: one level, no nesting, no comments.

use crate::util::{valid_date, valid_time};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub keys: Vec<(String, Value)>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Value::Bool(true)))
    }

    pub fn strings(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::List(xs)) => xs.iter().map(value_to_string).collect(),
            Some(v) => vec![value_to_string(v)],
            None => Vec::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(slot) = self.keys.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
        } else {
            self.keys.push((key.to_string(), value));
        }
    }

    pub fn push_unique(&mut self, key: &str, item: &str) {
        let mut items = self.strings(key);
        if !items.iter().any(|s| s == item) {
            items.push(item.to_string());
        }
        self.set(
            key,
            Value::List(items.into_iter().map(Value::String).collect()),
        );
    }

    pub fn remove_item(&mut self, key: &str, item: &str) {
        let items: Vec<Value> = self
            .strings(key)
            .into_iter()
            .filter(|s| s != item)
            .map(Value::String)
            .collect();
        if items.is_empty() {
            self.keys.retain(|(k, _)| k != key);
        } else {
            self.set(key, Value::List(items));
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        for (k, v) in &self.keys {
            out.push_str(k);
            match v {
                Value::List(xs) if flow_too_wide(xs) => {
                    out.push_str(":\n");
                    for item in xs {
                        out.push_str("  - ");
                        out.push_str(&render_scalar(item));
                        out.push('\n');
                    }
                }
                other => {
                    out.push_str(": ");
                    out.push_str(&render_value(other));
                    out.push('\n');
                }
            }
        }
        out.push_str("---\n");
        out
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::List(_) => String::new(),
    }
}

fn flow_too_wide(xs: &[Value]) -> bool {
    render_value(&Value::List(xs.to_vec())).chars().count() > 96
}

fn render_value(v: &Value) -> String {
    match v {
        Value::List(xs) => {
            let inner: Vec<String> = xs.iter().map(render_scalar).collect();
            format!("[{}]", inner.join(", "))
        }
        other => render_scalar(other),
    }
}

fn render_scalar(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::String(s) => {
            if needs_quotes(s) {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                s.clone()
            }
        }
        Value::List(_) => "[]".to_string(),
    }
}

fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s.starts_with(' ') || s.ends_with(' ') {
        return true;
    }
    if s == "true" || s == "false" || s == "null" || s == "~" {
        return true;
    }
    if s.starts_with('[') || s.starts_with('"') || s.starts_with('{') || s.starts_with('\'') {
        return true;
    }
    if s.contains([':', '#', '[', ']', ',', '{', '}']) {
        return true;
    }
    if s.parse::<i64>().is_ok() {
        return true;
    }
    false
}

#[derive(Debug)]
pub struct FmError {
    pub line: usize,
    pub message: String,
}

/// Split a record file into frontmatter and body. `body` keeps the source bytes.
pub fn split_record(text: &str) -> Result<(Frontmatter, String, usize), FmError> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| FmError {
            line: 1,
            message: "frontmatter must start with ---".into(),
        })?;
    let mut line_no = 2;
    let offset = text.len() - rest.len();
    let bytes = rest.as_bytes();
    let mut i = 0;
    let mut block = String::new();
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        let mut line = &rest[start..i];
        let had_cr = line.ends_with('\r');
        if had_cr {
            line = &line[..line.len() - 1];
        }
        let consumed = if i < bytes.len() { i + 1 } else { i };
        if line == "---" {
            let body = rest[consumed..].to_string();
            let fm = parse_block(&block)?;
            return Ok((fm, body, line_no + 1));
        }
        block.push_str(line);
        block.push('\n');
        i = consumed;
        line_no += 1;
        let _ = offset;
    }
    Err(FmError {
        line: 1,
        message: "frontmatter has no closing ---".into(),
    })
}

fn parse_block(block: &str) -> Result<Frontmatter, FmError> {
    let mut fm = Frontmatter::default();
    let lines: Vec<&str> = block.split('\n').collect();
    // split leaves a trailing empty from the newline we added; drop it.
    let lines: Vec<&str> = if lines.last() == Some(&"") {
        lines[..lines.len() - 1].to_vec()
    } else {
        lines
    };
    let mut n = 0;
    while n < lines.len() {
        let line_no = n + 2;
        let line = lines[n];
        if line.is_empty() {
            return Err(FmError {
                line: line_no,
                message: "blank line in frontmatter".into(),
            });
        }
        if line.starts_with('#') || line.starts_with(' ') || line.starts_with('\t') {
            return Err(FmError {
                line: line_no,
                message: format!("unsupported line '{line}'"),
            });
        }
        let Some((key, raw)) = line.split_once(':') else {
            return Err(FmError {
                line: line_no,
                message: format!("expected key: value, saw '{line}'"),
            });
        };
        // Digits are allowed because the deploy fields binary_sha256 and apk_sha256 contain them.
        let key_ok = !key.is_empty()
            && key.chars().next().unwrap().is_ascii_lowercase()
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !key_ok {
            return Err(FmError {
                line: line_no,
                message: format!("bad key '{key}'"),
            });
        }
        let raw = raw.trim();
        if raw.is_empty() {
            let mut items = Vec::new();
            n += 1;
            while n < lines.len() && lines[n].starts_with("  - ") {
                let item = lines[n][4..].trim();
                items.push(parse_scalar(item).map_err(|m| FmError {
                    line: n + 2,
                    message: m,
                })?);
                n += 1;
            }
            if items.is_empty() {
                return Err(FmError {
                    line: line_no,
                    message: format!("key '{key}' has no value"),
                });
            }
            fm.keys.push((key.to_string(), Value::List(items)));
            continue;
        }
        let value = parse_scalar(raw).map_err(|m| FmError {
            line: line_no,
            message: m,
        })?;
        fm.keys.push((key.to_string(), value));
        n += 1;
    }
    Ok(fm)
}

fn parse_scalar(raw: &str) -> Result<Value, String> {
    if raw == "true" {
        return Ok(Value::Bool(true));
    }
    if raw == "false" {
        return Ok(Value::Bool(false));
    }
    if raw.starts_with('[') {
        return parse_flow(raw);
    }
    if raw.starts_with('"') {
        return parse_quoted(raw).map(Value::String);
    }
    if raw.starts_with('\'') || raw.starts_with('{') || raw.starts_with('|') || raw.starts_with('>')
    {
        return Err(format!("unsupported value '{raw}'"));
    }
    if is_plain_int(raw) {
        return Ok(Value::Int(
            raw.parse().map_err(|_| format!("bad integer '{raw}'"))?,
        ));
    }
    if valid_time(raw) || valid_date(raw) {
        return Ok(Value::String(raw.to_string()));
    }
    if raw.contains(['#', '[', ']', '{', '}']) {
        return Err(format!("bare value '{raw}' needs quotes"));
    }
    Ok(Value::String(raw.to_string()))
}

fn is_plain_int(raw: &str) -> bool {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && !digits.starts_with('0')
        || digits == "0"
}

fn parse_quoted(raw: &str) -> Result<String, String> {
    if !raw.ends_with('"') || raw.len() < 2 {
        return Err(format!("unclosed quote in '{raw}'"));
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err("trailing escape".into()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn parse_flow(raw: &str) -> Result<Value, String> {
    if !raw.ends_with(']') {
        return Err(format!("unclosed list '{raw}'"));
    }
    let inner = &raw[1..raw.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Value::List(Vec::new()));
    }
    let mut items = Vec::new();
    let mut buf = String::new();
    let mut quoted = false;
    let mut escape = false;
    for c in inner.chars() {
        if escape {
            buf.push(c);
            escape = false;
            continue;
        }
        if c == '\\' && quoted {
            buf.push(c);
            escape = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
            buf.push(c);
            continue;
        }
        if c == ',' && !quoted {
            items.push(parse_scalar(buf.trim())?);
            buf.clear();
            continue;
        }
        buf.push(c);
    }
    if quoted {
        return Err(format!("unclosed quote in '{raw}'"));
    }
    if !buf.trim().is_empty() {
        items.push(parse_scalar(buf.trim())?);
    }
    Ok(Value::List(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_rule_header() {
        let src =
            "---\nid: RULE-4\ntype: rule\nareas: [web, i18n]\nlegacy: true\norder: 1\n---\nbody\n";
        let (fm, body, _) = split_record(src).unwrap();
        assert_eq!(body, "body\n");
        assert_eq!(fm.get_str("id"), Some("RULE-4"));
        assert_eq!(fm.strings("areas"), vec!["web", "i18n"]);
        assert!(fm.get_bool("legacy"));
        let again = format!("{}{body}", fm.render());
        let (fm2, body2, _) = split_record(&again).unwrap();
        assert_eq!(fm2.get_str("id"), Some("RULE-4"));
        assert_eq!(body2, body);
    }

    #[test]
    fn rejects_a_nested_key() {
        let src = "---\nmetadata:\n  type: project\n---\n";
        assert!(split_record(src).is_err());
    }
}
