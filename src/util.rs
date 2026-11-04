use std::time::{SystemTime, UNIX_EPOCH};

/// tok4 = ceil(Unicode scalar count / 4). The same unit for every budget.
pub fn tok4(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub fn suggest<'a>(word: &str, options: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for opt in options {
        let d = edit_distance(&word.to_lowercase(), &opt.to_lowercase());
        if d == 0 || d > 3 {
            continue;
        }
        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, opt));
        }
    }
    best.map(|(_, s)| s)
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

pub fn cut_words(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let budget = max_chars.saturating_sub(1);
    let mut end = 0;
    let mut last_space = None;
    for (i, c) in s.chars().enumerate() {
        // A space at the budget is a cut: the words before it fit, and the ellipsis uses the last slot.
        if c.is_whitespace() && i <= budget {
            last_space = Some(i);
        }
        if i >= budget {
            break;
        }
        end = i + 1;
    }
    let cut = last_space.filter(|i| *i > 0).unwrap_or(end);
    let mut owned: String = s.chars().take(cut).collect();
    while owned.ends_with(char::is_whitespace) {
        owned.pop();
    }
    owned.push('…');
    owned
}

pub fn valid_date(s: &str) -> bool {
    if s == "unknown" {
        return true;
    }
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let year: i32 = s[0..4].parse().ok().unwrap_or(0);
    let month: u32 = s[5..7].parse().ok().unwrap_or(0);
    let day: u32 = s[8..10].parse().ok().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return false;
    }
    let md = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if day > md[month as usize] {
        return false;
    }
    if month == 2 && day == 29 && !((year % 4 == 0 && year % 100 != 0) || year % 400 == 0) {
        return false;
    }
    true
}

pub fn valid_time(s: &str) -> bool {
    // YYYY-MM-DDTHH:MMZ
    if s.len() != 17 || !s.ends_with('Z') || s.as_bytes().get(10) != Some(&b'T') {
        return false;
    }
    if !valid_date(&s[..10]) || &s[..10] == "unknown" {
        return false;
    }
    let hh: u32 = s[11..13].parse().unwrap_or(99);
    let mm: u32 = s[14..16].parse().unwrap_or(99);
    s.as_bytes()[13] == b':' && hh <= 23 && mm <= 59
}

pub fn today_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil from days since 1970-01-01 (Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A temp name per process and per write: two writers of one file never share it.
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!(
        ".{name}.{}.{}.trailtmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// One writer per store. Every command that writes records, reservations or indexes holds
/// this file for the whole read-change-write, so two commands never both read the same
/// "next free number" or overwrite each other's `reservations.toml`.
pub struct Lock {
    path: std::path::PathBuf,
}

pub const LOCK_FILE: &str = ".trail.lock";

impl Lock {
    pub fn take(store_dir: &std::path::Path) -> Result<Lock, String> {
        use std::io::Write;
        let path = store_dir.join(LOCK_FILE);
        let wait = std::env::var("TRAIL_LOCK_WAIT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait);
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    let _ = writeln!(f, "pid {}", std::process::id());
                    return Ok(Lock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A writer that died leaves its lock behind. Nothing legitimate holds it
                    // for ten minutes.
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .map(|d| d.as_secs() > 600)
                        .unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "{} is held by another trailmark command; wait for it, or remove the file if none is running",
                            path.display()
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One `**` matches across directories, including the empty path.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn rec(p: &[u8], t: &[u8]) -> bool {
        let mut i = 0;
        let mut j = 0;
        while i < p.len() {
            if p[i] == b'*' && i + 1 < p.len() && p[i + 1] == b'*' {
                let mut rest = i + 2;
                if rest < p.len() && p[rest] == b'/' {
                    rest += 1;
                }
                if rec(&p[rest..], &t[j..]) {
                    return true;
                }
                while j < t.len() {
                    j += 1;
                    if rec(&p[rest..], &t[j..]) {
                        return true;
                    }
                }
                return false;
            }
            if p[i] == b'*' {
                i += 1;
                loop {
                    if rec(&p[i..], &t[j..]) {
                        return true;
                    }
                    if j >= t.len() || t[j] == b'/' {
                        return false;
                    }
                    j += 1;
                }
            }
            if j >= t.len() {
                return false;
            }
            if p[i] == b'?' {
                if t[j] == b'/' {
                    return false;
                }
                i += 1;
                j += 1;
                continue;
            }
            if p[i] != t[j] {
                return false;
            }
            i += 1;
            j += 1;
        }
        j == t.len()
    }
    for alt in expand_braces(pattern) {
        if rec(alt.as_bytes(), text.as_bytes()) {
            return true;
        }
    }
    false
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close) = pattern[open..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + close;
    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let mut out = Vec::new();
    for alt in pattern[open + 1..close].split(',') {
        for rest in expand_braces(suffix) {
            out.push(format!("{prefix}{alt}{rest}"));
        }
    }
    out
}

pub fn heading_slug(heading: &str) -> String {
    let t = heading.trim().trim_start_matches('#').trim();
    slug(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tok_rounds_up() {
        assert_eq!(tok4(""), 0);
        assert_eq!(tok4("abcd"), 1);
        assert_eq!(tok4("abcde"), 2);
    }

    #[test]
    fn glob_covers_nested_and_flat() {
        assert!(glob_match("server/**/*.rs", "server/src/main.rs"));
        assert!(glob_match("server/**/*.rs", "server/main.rs"));
        assert!(glob_match("pkg/**/*.{ts,tsx}", "pkg/a/b.tsx"));
        assert!(!glob_match("server/**/*.rs", "web/src/main.rs"));
    }

    #[test]
    fn cut_keeps_a_word() {
        assert_eq!(cut_words("one two three", 8), "one two…");
    }
}
