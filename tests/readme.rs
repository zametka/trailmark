//! The README's Five minutes and Commands sections, run line by line through `sh` with the real
//! binary on `PATH`, the way a reader pastes them. Both sections drifted from the binary once;
//! this is what stops that. The bakery block is left out: it starts with a network clone.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn empty_dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("trail-readme-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn readme() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap()
}

/// The text under `## <heading>`, up to the next `## `.
fn section(text: &str, heading: &str) -> String {
    let start = text
        .find(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("no section {heading}"))
        + 1;
    let rest = &text[start + 3..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].to_string()
}

/// The lines of a section's first fenced block.
fn first_block(section: &str) -> Vec<String> {
    section
        .split("```")
        .nth(1)
        .expect("a fenced block")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// The second cell of each table row, without its backticks and with `\|` as the reader sees it.
fn table_commands(section: &str) -> Vec<String> {
    section
        .lines()
        .filter(|l| l.starts_with("| `"))
        .map(|l| {
            let cell = l.split("` | `").nth(1).expect("two cells");
            cell.trim_end_matches("` |").replace("\\|", "|")
        })
        .collect()
}

fn sh(dir: &Path, line: &str) -> Output {
    let bin = Path::new(env!("CARGO_BIN_EXE_trailmark")).parent().unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("sh")
        .args(["-c", line])
        .current_dir(dir)
        .env("PATH", path)
        .env_remove("TRAIL_ROOT")
        .env_remove("TRAIL_ADAPTERS")
        .output()
        .expect("sh runs")
}

fn run_all(dir: &Path, lines: &[String]) {
    for line in lines {
        let out = sh(dir, line);
        assert!(
            out.status.success(),
            "{line}\nexit {:?}\nstdout:\n{}\nstderr:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn five_minutes_runs_as_written_and_ends_clean() {
    let dir = empty_dir("five");
    let text = readme();
    let five = section(&text, "Five minutes");
    run_all(&dir.0, &first_block(&five));
    let out = sh(&dir.0, "trailmark check");
    let summary = "0 errors, 0 warnings, 2 records, 1 code references";
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        summary,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(five.contains(summary), "the README prints the same line");
}

#[test]
fn the_commands_table_runs_in_order_from_five_minutes() {
    let dir = empty_dir("table");
    let text = readme();
    run_all(&dir.0, &first_block(&section(&text, "Five minutes")));
    let commands = section(&text, "Commands");
    run_all(&dir.0, &first_block(&commands));
    let mut rows = table_commands(&commands);
    assert!(rows.len() >= 18, "{rows:?}");
    // Five minutes ran it, and a second `init` is refused on purpose.
    assert_eq!(rows.remove(0), "trailmark init");
    for name in ["close", "figure", "why", "set", "terms"] {
        let prefix = format!("trailmark {name} ");
        assert!(
            rows.iter()
                .any(|r| r.starts_with(&prefix) || r == prefix.trim_end()),
            "no row for {name}: {rows:?}"
        );
    }
    run_all(&dir.0, &rows);
}
