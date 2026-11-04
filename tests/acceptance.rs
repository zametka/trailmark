//! Fixture store and the checker defects. The root is a temp directory, never this repo.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Root {
    path: PathBuf,
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn trailmark(root: &Path, args: &[&str]) -> trailmark::Outcome {
    let mut v = vec!["--root".to_string(), root.display().to_string()];
    v.extend(args.iter().map(|s| (*s).to_string()));
    trailmark::run(&v, "")
}

fn expect(out: &trailmark::Outcome, code: i32, what: &str) {
    assert_eq!(
        out.code, code,
        "{what}\nstdout:\n{}\nstderr:\n{}",
        out.stdout, out.stderr
    );
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn replace(path: &Path, from: &str, to: &str) {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {}", path.display()));
    assert!(text.contains(from), "missing {from} in {}", path.display());
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

/// Every reservation a store directory holds, as one text: the legacy `reservations.toml`
/// and each `reservations/<owner>.toml`.
fn reservations_text(store: &Path) -> String {
    let mut text = std::fs::read_to_string(store.join("reservations.toml")).unwrap_or_default();
    if let Ok(entries) = std::fs::read_dir(store.join("reservations")) {
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for file in files {
            text.push_str(&std::fs::read_to_string(file).unwrap());
        }
    }
    text
}

fn fixture() -> Root {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("trail-fx-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    write_tree(&path);
    Root { path }
}

fn write_tree(root: &Path) {
    write(
        root,
        "trail.toml",
        r#"name = "Fixture"
requires = ">=0.1.0"
store = "store"
cutover = false
frozen = false
review_step = "STEP-4"

[ids]
rule = "GUIDE"
charter = "GUIDE"
charter_also = ["STEP"]
rule_style = "section"

[budgets]
core = 2500
context_default = 8000
context_max = 20000
rules_area_index = 3000
hot_rules = 1200
open_index = 2500
decisions_index = 3000
loops_index = 1500
deploys_index = 800
items_index = 4000
record_warn = { rule = 2000, loop = 10000, review = 10000 }

[reserve]
rule = 10
open = 10

[areas]
web = { keywords = ["css", "layout", "translation"], paths = ["web/**"] }
i18n = { keywords = ["locale", "translation"], paths = ["locales/**"] }
tests = { keywords = ["fixture", "assert"], paths = ["tests/**"] }
mobile = { keywords = ["handset"], paths = ["mobile/**"] }
security = { keywords = ["token"], paths = [] }
deploy = { keywords = ["rollback"], paths = [] }
process = { keywords = ["review", "author", "loop"], paths = [] }

[code]
include = ["code/**/*.ts"]
exclude = ["**/target/**"]

[[alias_pattern]]
match = '§ ?2\.(\d+[a-z]?)'
id = "GUIDE-2.$1"
[[alias_pattern]]
match = '§ ?2\.(\d+)\s*[–-]\s*(?:§ ?2\.)?(\d+)'
range = "GUIDE-2.$1..GUIDE-2.$2"
[[alias_pattern]]
match = 'Review of ([A-Z]{1,4}\d+[a-z]?)'
id = "REV-$1"
[[alias_pattern]]
match = '(?:deploy )?#(\d{1,4})\b'
id = "DEP-$1"
context_only = true

[[measure.task]]
name = "bake loop"
text = "weigh loaf"
areas = ["web", "i18n"]
"#,
    );
    write(
        root,
        "code/note.ts",
        "// The loaf is weighed first (GUIDE-2.1).\n",
    );
    write(
        root,
        "store/reservations.toml",
        r#"[[reservation]]
type = "rule"
from = 100
to = 109
for = "LOOP-BK2"
date = "2026-10-27"

[[reservation]]
type = "open"
from = 214
to = 223
for = "LOOP-BK2"
date = "2026-10-27"
"#,
    );
    write(
        root,
        "store/charter/GUIDE-1.md",
        r#"---
id: GUIDE-1
type: charter
title: How we work
date: 2026-10-24
status: active
pin: true
---
- Weigh a loaf before it is wrapped.
- Name every flour on the label.
- A standing order changes only when the customer says so.
"#,
    );
    write(
        root,
        "store/charter/STEP-2.md",
        r#"---
id: STEP-2
type: charter
title: Brief — restate the ask before building
date: 2026-10-24
status: active
---
Restate the ask before building.
"#,
    );
    write(
        root,
        "store/charter/STEP-4.md",
        r#"---
id: STEP-4
type: charter
title: Review — read it and run it
date: 2026-10-24
status: active
---
Read the diff and run the failing case before the next loop.
"#,
    );
    write(
        root,
        "store/rules/2.1.md",
        r#"---
id: GUIDE-2.1
type: rule
title: Sourdough loaves are weighed and wrapped
short: Sourdough loaves are weighed and wrapped
date: 2026-10-24
status: active
legacy: true
areas: [web, i18n]
source: LOOP-BK1
---
### 2.1 Sourdough loaves are weighed and wrapped

**Incident (2026-10-24, BK1).** A sourdough left the counter light.

> **Rule.** Sourdough loaves are weighed and wrapped.
"#,
    );
    write(
        root,
        "store/rules/2.2.md",
        r#"---
id: GUIDE-2.2
type: rule
title: A review names the failing case
short: A review names the failing case
date: 2026-10-24
status: active
legacy: true
areas: [tests]
source: REV-BK1
---
### 2.2 A review names the failing case

**Incident (2026-10-24).** The review said it was fine and the case still failed.

> **Rule.** A review names the failing case.
"#,
    );
    write(
        root,
        "store/rules/2.3.md",
        r#"---
id: GUIDE-2.3
type: rule
title: Old layer rule
short: Old layer rule
date: 2026-10-20
status: superseded
legacy: true
areas: [tests]
source: unknown
superseded_by: [GUIDE-2.4]
---
### 2.3 Old layer rule

**Incident (2026-10-20).** A weight was written down at the wrong bench.

> **Rule.** The old wording, kept so the link still resolves.
"#,
    );
    write(
        root,
        "store/rules/2.4.md",
        r#"---
id: GUIDE-2.4
type: rule
title: Weigh a loaf on the scale that baked it
short: Weigh a loaf on the scale that baked it
date: 2026-10-24
status: active
legacy: true
areas: [tests]
source: REV-BK1
supersedes: [GUIDE-2.3]
---
### 2.4 Weigh a loaf on the scale that baked it

**Incident (2026-10-24).** The weight was checked on a scale that had not baked the loaf.

> **Rule.** Weigh a loaf on the scale that baked it.
"#,
    );
    write(
        root,
        "store/rules/2.100.md",
        r#"---
id: GUIDE-2.100
type: rule
title: A tray fits on the rack
short: A tray fits on the rack
date: 2026-10-27
status: active
areas: [web, i18n]
source: LOOP-BK2
---
### 2.100 A tray fits on the rack

**Incident (2026-10-27).** The morning tray did not fit the rack.

> **Rule.** A sourdough tray fits on the rack.
"#,
    );
    write(
        root,
        "store/decisions/0001.md",
        r#"---
id: DEC-0001
type: decision
title: Keep the shop name on the bag
date: 2026-10-24
status: active
pin: true
decided_by: author
basis: word
quote: The shop name stays.
scope: product
---
## Decision

Keep the shop name on the bag.

## Why

Old bags are still in cupboards.
"#,
    );
    write(
        root,
        "store/decisions/0031.md",
        r#"---
id: DEC-0031
type: decision
title: Rye stays on the standing menu
date: 2026-10-24
status: active
decided_by: author
basis: silence
scope: process
---
## Decision

Rye stays on the standing menu.

## Why

Nobody objected when rye was proposed.
"#,
    );
    write(
        root,
        "store/discussions/0418.md",
        r#"---
id: DSC-0418
type: discussion
title: One tray on the rack
date: 2026-10-27
status: triaged
from: author
items: [ITEM-BK2]
---
### 1

**Ruling:** keep one tray.

**Goes to:** ITEM-BK2

### 2

**Ruling:** check it on the rack.
"#,
    );
    write(
        root,
        "store/items/BK1.md",
        r#"---
id: ITEM-BK1
type: item
title: Morning bake
date: 2026-10-24
status: done
asked_by: author
platform: [web]
loops: [LOOP-BK1]
---
The morning loaf left light, and the scale was the cause.
"#,
    );
    write(
        root,
        "store/items/BK2.md",
        r#"---
id: ITEM-BK2
type: item
title: One tray on the rack
date: 2026-10-27
status: running
order: 1
asked_by: author
platform: [web]
---
One tray has to fit the rack, including the longest loaf name.
"#,
    );
    write(
        root,
        "store/loops/BK1.md",
        r#"---
id: LOOP-BK1
type: loop
title: Morning bake
date: 2026-10-24
status: done
item: ITEM-BK1
platform: [web]
started: 2026-10-24
closed: 2026-10-24
gates: ["web tests passed"]
learned: [GUIDE-2.1]
review: REV-BK1
source: DSC-0418
---
## Brief

The sourdough loaf left lighter than the ticket. It was weighed at the pass.

## Built

The scale sits at the pass.

## Verified

The weighed loaf matches the ticket.

## Gates

The morning count matched.

## Not verified

A second oven.

## Lessons

A loaf is weighed before it is wrapped.
"#,
    );
    write(
        root,
        "store/loops/BK2.md",
        r#"---
id: LOOP-BK2
type: loop
title: One tray on the rack
date: 2026-10-27
status: running
item: ITEM-BK2
platform: [web]
started: 2026-10-27
source: DSC-0418
gates: ["pending"]
---
## Brief

One tray has to fit the rack, including the longest loaf name.

## Built

Pending.

## Verified

Pending.

## Gates

Pending.

## Not verified

A second oven.

## Lessons

Pending.
"#,
    );
    write(
        root,
        "store/reviews/BK1.md",
        r#"---
id: REV-BK1
type: review
title: Review of the morning bake
date: 2026-10-24
status: done
reviews: [LOOP-BK1]
verdict: go-with-fixes
gates: ["web tests passed"]
learned: [GUIDE-2.2]
---
## Verdict

go-with-fixes

## Findings

The scale still fails one loaf.

### F1 — the loaf was light

**Severity:** medium
**Was:** the first loaf was right.
**Now:** the second loaf was light.
**Failing case:** weigh_test
**Rule:** learned GUIDE-2.2

## Read and run, no change

Read the diff and ran weigh_test.

## Gates

The morning count matched.

## Not verified

A second oven.
"#,
    );
    write(
        root,
        "store/open/0205.md",
        r#"---
id: OPEN-0205
type: open
title: Retire the spare oven log
date: 2026-10-24
status: open
legacy: true
kind: owed-step
severity: low
owner: author
opened_by: REV-BK1
areas: [security]
---
## What

The spare log is still on the pass.

## How to close

Take it off the pass and say so.
"#,
    );
    write(
        root,
        "store/open/0204.md",
        r#"---
id: OPEN-0204
type: open
title: Log the oven after the bake
date: 2026-10-24
status: closed
legacy: true
kind: follow-up
severity: low
owner: next-loop
opened_by: LOOP-BK1
closed_by: LOOP-BK1
areas: [web]
---
## What

The oven log was filled in after the bake.

## How to close

Closed by the loop that moved the log.
"#,
    );
    write(
        root,
        "store/open/0214.md",
        r#"---
id: OPEN-0214
type: open
title: The Sunday van was not loaded
date: 2026-10-27
status: open
kind: unverified
severity: medium
owner: next-loop
opened_by: LOOP-BK2
areas: [mobile]
---
## What

The Sunday van was not loaded.

## How to close

Load one Sunday van and write down what happened.
"#,
    );
    write(
        root,
        "store/deploys/4.md",
        r#"---
id: DEP-4
type: deploy
title: Ship the till
date: 2026-10-24
status: live
target: web
time: 2026-04-12T15:00Z
authorized_by: author
word: ship it
word_date: 2026-10-24
backup: none
backup_word: no backup
commit: abcdef01
binary_sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
migrations: none
carries: [LOOP-BK1]
rollback: ["put the previous binary back"]
---
## Verified

The till opened.

## Not verified

A Sunday van was not checked.
"#,
    );
    write(
        root,
        "store/facts/gate-web.md",
        r#"---
id: FACT-gate-web
type: fact
title: The web gate fails closed
date: 2026-10-24
status: active
kind: trap
scope: repo
applies_to: [web]
---
**Why:** a gate that fails open hides a broken page.

**How to apply:** run the gate before the edit and stop when it fails.
"#,
    );
    write(
        root,
        "store/incidents/0003.md",
        r#"---
id: INC-0003
type: incident
title: A label wrapped
date: 2026-10-24
status: resolved
severity: low
production: true
found_by: LOOP-BK1
earned: [GUIDE-2.2]
---
## What happened

A label wrapped once the loaf name was long.

## How it was found

A review ran the bag case.

## Impact

One bag.

## What changed

The rule now names the longest loaf name.
"#,
    );
}

#[test]
fn clean_store_indexes_and_checks() {
    let root = fixture();
    let first = trailmark(&root.path, &["index"]);
    expect(&first, 0, "index");
    let core = std::fs::read(root.path.join("store/index/core.md")).unwrap();
    let again = trailmark(&root.path, &["index"]);
    expect(&again, 0, "index again");
    let core2 = std::fs::read(root.path.join("store/index/core.md")).unwrap();
    assert_eq!(core, core2, "index bytes changed between runs");
    let same = trailmark(&root.path, &["index", "--check"]);
    expect(&same, 0, "index --check");
    let check = trailmark(&root.path, &["check"]);
    expect(&check, 0, "check");
    assert!(check.stdout.contains("0 errors"), "{}", check.stdout);
    let text = String::from_utf8(core).unwrap();
    assert!(
        text.contains("Production DEP-4 (2026-10-24, abcdef01, none, rollback: put the previous binary back)."),
        "{text}"
    );
    assert!(text.contains("Running: LOOP-BK2, ITEM-BK2."), "{text}");
    assert!(text.contains("Owed by the author: 1 open item"), "{text}");
}

#[test]
fn show_part_matches_the_golden_file() {
    let root = fixture();
    let out = trailmark(&root.path, &["show", "GUIDE-2.1", "--part", "rule"]);
    expect(&out, 0, "show");
    let golden = include_str!("golden/show-rule.txt");
    assert_eq!(out.stdout, golden);
}

#[test]
fn find_is_stable_and_ranks_the_layout_rule_first() {
    let root = fixture();
    let a = trailmark(&root.path, &["find", "weighed"]);
    let b = trailmark(&root.path, &["find", "weighed"]);
    expect(&a, 0, "find");
    assert_eq!(a.stdout, b.stdout);
    let first = a.stdout.lines().next().unwrap_or("");
    assert!(first.starts_with("GUIDE-2.1"), "{first}\n{}", a.stdout);
}

#[test]
fn context_stays_inside_the_budget() {
    let root = fixture();
    let indexed = trailmark(&root.path, &["index"]);
    expect(&indexed, 0, "index");
    let out = trailmark(&root.path, &["context", "weigh loaf"]);
    expect(&out, 0, "context");
    let line = out.stdout.lines().next().unwrap_or("");
    let tok: usize = line
        .split("tok4=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.trim_end_matches("-->").parse().ok())
        .unwrap_or(usize::MAX);
    let budget: usize = line
        .split("budget=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert!(tok <= budget && budget == 8000, "{line}");
}

#[test]
fn why_walks_from_the_code_citation() {
    let root = fixture();
    let out = trailmark(&root.path, &["why", "code/note.ts"]);
    expect(&out, 0, "why");
    assert!(out.stdout.contains("GUIDE-2.1"), "{}", out.stdout);
    assert!(out.stdout.contains("LOOP-BK1"), "{}", out.stdout);
}

#[test]
fn workflow_mints_reserves_and_refuses_a_deploy_without_a_word() {
    let root = fixture();
    let rule = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Keep the grid on one line",
            "--areas",
            "web",
        ],
    );
    expect(&rule, 0, "add rule");
    assert!(rule.stdout.contains("GUIDE-2.101"), "{}", rule.stdout);
    assert!(root.path.join("store/rules/2.101.md").is_file());
    let loop_text = std::fs::read_to_string(root.path.join("store/loops/BK2.md")).unwrap();
    assert!(loop_text.contains("GUIDE-2.101"), "{loop_text}");

    let open = trailmark(
        &root.path,
        &[
            "add",
            "open",
            "--for",
            "LOOP-BK2",
            "--title",
            "Look at the oven",
            "--areas",
            "mobile",
            "--kind",
            "unverified",
            "--severity",
            "low",
            "--owner",
            "next-loop",
        ],
    );
    expect(&open, 0, "add open");
    assert!(open.stdout.contains("OPEN-0215"), "{}", open.stdout);

    let review = trailmark(
        &root.path,
        &[
            "add",
            "review",
            "--for",
            "LOOP-BK2",
            "--title",
            "Review the tray",
            "--verdict",
            "go-with-fixes",
            "--field",
            "gates=web tests passed",
        ],
    );
    expect(&review, 0, "add review");
    assert!(review.stdout.contains("REV-BK2"), "{}", review.stdout);
    let reserved = reservations_text(&root.path.join("store"));
    assert!(reserved.contains("from = 110"), "{reserved}");
    assert!(reserved.contains("REV-BK2"), "{reserved}");
    let loop_text = std::fs::read_to_string(root.path.join("store/loops/BK2.md")).unwrap();
    assert!(loop_text.contains("REV-BK2"), "{loop_text}");

    let denied = trailmark(
        &root.path,
        &[
            "add",
            "deploy",
            "86",
            "--title",
            "The next ship",
            "--field",
            "target=web",
            "--field",
            "time=2026-10-26T12:00Z",
            "--field",
            "authorized_by=author",
            "--field",
            "word_date=2026-10-26",
            "--field",
            "backup=none",
            "--field",
            "backup_word=no backup",
            "--field",
            "commit=abcdef1",
            "--field",
            "binary_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--field",
            "migrations=76/76",
            "--field",
            "carries=LOOP-BK1",
            "--field",
            "rollback=put the previous binary back",
            "--field",
            "previous=DEP-4",
            "--field",
            "date=2026-10-26",
        ],
    );
    expect(&denied, 6, "deploy without word");
    assert!(denied.stderr.contains("no 'word'"), "{}", denied.stderr);
    assert!(!root.path.join("store/deploys/86.md").exists());

    let shipped = trailmark(
        &root.path,
        &[
            "add",
            "deploy",
            "86",
            "--title",
            "The next ship",
            "--field",
            "target=web",
            "--field",
            "time=2026-10-26T12:00Z",
            "--field",
            "authorized_by=author",
            "--field",
            "word=ship it",
            "--field",
            "word_date=2026-10-26",
            "--field",
            "backup=none",
            "--field",
            "backup_word=no backup",
            "--field",
            "commit=abcdef1",
            "--field",
            "binary_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--field",
            "migrations=76/76",
            "--field",
            "carries=LOOP-BK1",
            "--field",
            "rollback=put the previous binary back",
            "--field",
            "previous=DEP-4",
            "--field",
            "date=2026-10-26",
        ],
    );
    expect(&shipped, 0, "deploy");
    assert!(root.path.join("store/deploys/86.md").is_file());
}

#[test]
fn reservation_exhaustion_and_a_frozen_store() {
    let root = fixture();
    replace(
        &root.path.join("store/reservations.toml"),
        "to = 109",
        "to = 100",
    );
    let out = trailmark(
        &root.path,
        &[
            "add", "rule", "--for", "LOOP-BK2", "--title", "No room", "--areas", "web",
        ],
    );
    expect(&out, 5, "exhausted");
    assert!(
        out.stderr
            .contains("ask the orchestrator for `trailmark reserve rule 10 --for LOOP-BK2`"),
        "{}",
        out.stderr
    );

    let frozen = fixture();
    replace(
        &frozen.path.join("trail.toml"),
        "frozen = false",
        "frozen = true",
    );
    let out = trailmark(
        &frozen.path,
        &[
            "add", "rule", "--for", "LOOP-BK2", "--title", "No", "--areas", "web",
        ],
    );
    expect(&out, 7, "frozen");
}

fn prepared() -> Root {
    let root = fixture();
    let out = trailmark(&root.path, &["index"]);
    expect(&out, 0, "index");
    root
}

#[test]
fn c01_unknown_key() {
    let root = prepared();
    replace(
        &root.path.join("store/rules/2.100.md"),
        "areas: [web, i18n]\n",
        "area: web\nareas: [web, i18n]\n",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C01"), "{}", out.stdout);
    assert!(
        out.stdout.contains("did you mean 'areas'"),
        "{}",
        out.stdout
    );
    assert_eq!(out.code, 1);
}

#[test]
fn c02_bad_enum() {
    let root = prepared();
    replace(
        &root.path.join("store/open/0214.md"),
        "kind: unverified",
        "kind: todo",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C02"), "{}", out.stdout);
    assert!(out.stdout.contains("kind 'todo'"), "{}", out.stdout);
}

#[test]
fn c03_id_does_not_match_the_file() {
    let root = prepared();
    replace(
        &root.path.join("store/rules/2.100.md"),
        "id: GUIDE-2.100",
        "id: GUIDE-2.101",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout
            .contains("id GUIDE-2.101 does not match file 2.100.md"),
        "{}",
        out.stdout
    );
}

#[test]
fn c04_duplicate_id() {
    let root = prepared();
    let text = std::fs::read_to_string(root.path.join("store/rules/2.1.md")).unwrap();
    write(&root.path, "store/rules/2.1-copy.md", &text);
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C04"), "{}", out.stdout);
    assert!(
        out.stdout.contains("GUIDE-2.1 defined twice"),
        "{}",
        out.stdout
    );
}

#[test]
fn c05_dead_link() {
    let root = prepared();
    replace(
        &root.path.join("store/loops/BK2.md"),
        "source: DSC-0418\n",
        "source: DSC-0418\nlinks: [OPEN-0999]\n",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("dead link OPEN-0999 (field links)"),
        "{}",
        out.stdout
    );
}

#[test]
fn a_fact_id_may_be_45_characters_and_is_cited_whole() {
    let root = prepared();
    // 45 characters: FACT- and a 40-character slug.
    let long = "FACT-a-fact-key-that-runs-to-forty-characters";
    assert_eq!(long.len(), 45);
    let fact = |id: &str| {
        format!(
            "---\nid: {id}\ntype: fact\ntitle: A long fact\ndate: 2026-10-26\nstatus: active\nkind: trap\nscope: repo\n---\n**Why:** it is long.\n\n**How to apply:** cite it whole.\n"
        )
    };
    write(
        &root.path,
        &format!("store/facts/{}.md", &long["FACT-".len()..]),
        &fact(long),
    );
    replace(
        &root.path.join("store/loops/BK2.md"),
        "source: DSC-0418\n",
        &format!("source: DSC-0418\nlinks: [{long}]\n"),
    );
    // A body that cites a hyphenated fact resolves it whole, not as `FACT-a`.
    let pf2 = root.path.join("store/loops/BK2.md");
    let mut text = std::fs::read_to_string(&pf2).unwrap();
    text.push_str(&format!("\nSee {long} before the edit.\n"));
    std::fs::write(&pf2, text).unwrap();
    let idx = trailmark(&root.path, &["index"]);
    expect(&idx, 0, "index with a 45-character fact");
    let out = trailmark(&root.path, &["check"]);
    expect(
        &out,
        0,
        "a 45-character fact id checks, cited in a field and a body",
    );
    let shown = trailmark(&root.path, &["show", long]);
    expect(&shown, 0, "show the long fact");
    // 46 characters is one too many for a fact; 25 is one too many for anything else.
    let over = "FACT-a-fact-key-that-runs-to-forty-one-characters";
    assert!(over.len() > 45);
    write(
        &root.path,
        &format!("store/facts/{}.md", &over["FACT-".len()..]),
        &fact(over),
    );
    let bad = trailmark(&root.path, &["check"]);
    assert!(bad.stdout.contains("C03"), "{}", bad.stdout);
    assert!(trailmark(&root.path, &["show", "DEC-00000000000000000001"]).code != 0);
}

#[test]
fn a_local_record_stays_out_of_the_committed_indexes() {
    // A record under local/ is gitignored: a clone or a worktree does not have it. The
    // indexes are committed, so they must read the same with it and without it.
    let root = prepared();
    let core = std::fs::read(root.path.join("store/index/core.md")).unwrap();
    let ids = std::fs::read(root.path.join("store/index/ids.tsv")).unwrap();
    write(
        &root.path,
        "store/local/facts/trays.md",
        "---\nid: FACT-trays\ntype: fact\ntitle: The spare trays in the rack\ndate: 2026-10-26\nstatus: active\nkind: device\nscope: local\n---\n**Why:** tray counts differ per shop.\n\n**How to apply:** read them here, cite GUIDE-2.1 there.\n",
    );
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "a new local record leaves the indexes fresh");
    let idx = trailmark(&root.path, &["index"]);
    expect(&idx, 0, "index with a local record");
    assert_eq!(
        std::fs::read(root.path.join("store/index/core.md")).unwrap(),
        core
    );
    assert_eq!(
        std::fs::read(root.path.join("store/index/ids.tsv")).unwrap(),
        ids
    );
    let shown = trailmark(&root.path, &["show", "FACT-trays"]);
    expect(&shown, 0, "a local record is still shown where it exists");
    std::fs::remove_dir_all(root.path.join("store/local")).unwrap();
    let clone = trailmark(&root.path, &["check"]);
    expect(&clone, 0, "the indexes stay fresh without the local record");
}

#[test]
fn c06_heading_outside_the_store() {
    let root = prepared();
    write(
        &root.path,
        "extra.md",
        "\
# Extra

### 2.1 Sourdough loaves are weighed and wrapped

```
### 2.50 Inside a fence
```

### 2.50 A heading that is not a record

**Incident (2026-10-26).** Appended to the old file after the migration.

> **Rule.** A rule written in the wrong place.

### 2.1 Setup, a numbered section in a design note
",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("extra.md:9:"),
        "{}\n{}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("C06"), "{}", out.stdout);
    // A numbered section with no incident or Rule block is not rule-shaped.
    assert!(!out.stdout.contains("extra.md:15:"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("extra.md:6:"),
        "fenced heading was flagged\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("extra.md:3:"),
        "an existing first line was flagged\n{}",
        out.stdout
    );
}

#[test]
fn c07_rule_block_removed() {
    let root = prepared();
    replace(
        &root.path.join("store/rules/2.100.md"),
        "> **Rule.** A sourdough tray fits on the rack.\n",
        "",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("no '> **Rule.**' block"),
        "{}",
        out.stdout
    );
}

#[test]
fn c08_stale_index() {
    let root = prepared();
    // C08 holds where the indexes are committed; by default they are a local cache.
    replace(
        &root.path.join("trail.toml"),
        "requires = \">=0.1.0\"\n",
        "requires = \">=0.1.0\"\ncommit_index = true\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let path = root.path.join("store/index/open.md");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\nhand edit\n");
    std::fs::write(&path, text).unwrap();
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C08"), "{}", out.stdout);
    assert!(
        out.stdout.contains("index/open.md is stale"),
        "{}",
        out.stdout
    );
}

#[test]
fn c09_over_budget() {
    let root = fixture();
    replace(
        &root.path.join("trail.toml"),
        "rules_area_index = 3000",
        "rules_area_index = 1",
    );
    let out = trailmark(&root.path, &["check"]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(out.stdout.contains("C09"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("file says missing"),
        "missing indexes were reported with the budget failure\n{}",
        out.stdout
    );
}

#[test]
fn c10_code_cites_a_missing_rule() {
    let root = prepared();
    write(
        &root.path,
        "code/missing.ts",
        "export const note = \"see §2.999\";\n",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C10"), "{}", out.stdout);
    assert!(
        out.stdout.contains("§2.999 resolves to nothing"),
        "{}",
        out.stdout
    );
}

#[test]
fn c11_number_minted_in_another_block() {
    let root = prepared();
    replace(
        &root.path.join("store/rules/2.100.md"),
        "source: LOOP-BK2",
        "source: LOOP-BK3",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout
            .contains("GUIDE-2.100 minted by LOOP-BK3 inside LOOP-BK2's block"),
        "{}",
        out.stdout
    );
}

#[test]
fn c12_deploy_without_a_word() {
    let root = prepared();
    replace(&root.path.join("store/deploys/4.md"), "word: ship it\n", "");
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("no 'word'"), "{}", out.stdout);
}

#[test]
fn c13_deleted_id_in_history() {
    let root = prepared();
    git(&root.path, &["init"]);
    write(
        &root.path,
        "store/decisions/0099.md",
        "---\nid: DEC-0099\ntype: decision\ntitle: Gone\ndate: 2026-10-24\nstatus: active\n---\n## Decision\n\nGone.\n",
    );
    git(&root.path, &["add", "store/decisions/0099.md"]);
    git(&root.path, &["commit", "-m", "add DEC-0099"]);
    std::fs::remove_file(root.path.join("store/decisions/0099.md")).unwrap();
    git(&root.path, &["add", "-u"]);
    git(&root.path, &["commit", "-m", "drop DEC-0099"]);
    let out = trailmark(&root.path, &["check", "--history"]);
    assert!(out.stdout.contains("C13"), "{}\n{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("DEC-0099 existed at"), "{}", out.stdout);
}

#[test]
fn c14_oversize_record_is_a_warning() {
    let root = prepared();
    replace(&root.path.join("trail.toml"), "rule = 2000", "rule = 1");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "warnings stay warnings");
    assert!(out.stdout.contains("C14"), "{}", out.stdout);
}

#[test]
fn c15_code_cites_a_superseded_rule() {
    let root = prepared();
    write(
        &root.path,
        "code/old.ts",
        "export const old = \"GUIDE-2.3\";\n",
    );
    let indexed = trailmark(&root.path, &["index"]);
    expect(&indexed, 0, "reindex after the new citation");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "C15 is a warning");
    assert!(out.stdout.contains("C15"), "{}", out.stdout);
    assert!(out.stdout.contains("GUIDE-2.3"), "{}", out.stdout);
    // A record line that names the old rule and the one that replaced it tells the story of
    // the replacement; a line naming the old one alone is stale.
    write(
        &root.path,
        "store/discussions/0050.md",
        "---\nid: DSC-0050\ntype: discussion\ntitle: Notes\ndate: 2026-10-27\nstatus: open\nfrom: author\n---\nGUIDE-2.3 was superseded by GUIDE-2.4.\n\nKeep GUIDE-2.3 in mind.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["check"]);
    let notes: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("C15") && l.contains("0050.md"))
        .collect();
    assert_eq!(notes.len(), 1, "{}", out.stdout);
    assert!(notes[0].contains("0050.md:11"), "{}", out.stdout);
}

#[test]
fn c16_too_many_pins() {
    let root = prepared();
    for n in 1..=39 {
        let id = format!("GUIDE-c{n:02}");
        write(
            &root.path,
            &format!("store/charter/{id}.md"),
            &format!("---\nid: {id}\ntype: charter\ntitle: Pin {n}\ndate: 2026-10-24\nstatus: active\npin: true\n---\nA pinned line.\n"),
        );
    }
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C16"), "{}", out.stdout);
    assert!(out.stdout.contains("41 pinned"), "{}", out.stdout);
}

#[test]
fn c17_unknown_commit_inside_an_own_repo() {
    let root = prepared();
    git(&root.path, &["init"]);
    std::fs::write(root.path.join("README.txt"), "fixture\n").unwrap();
    git(&root.path, &["add", "README.txt"]);
    git(&root.path, &["commit", "-m", "init"]);
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "C17 is a warning");
    assert!(out.stdout.contains("C17"), "{}", out.stdout);
    assert!(out.stdout.contains("abcdef01"), "{}", out.stdout);
}

#[test]
fn c18_only_when_cutover_is_set() {
    let root = prepared();
    let mut long = String::from("# Long\n");
    for n in 0..50 {
        long.push_str(&format!("line {n}\n"));
    }
    write(&root.path, "docs/long.md", &long);
    write(
        &root.path,
        "store/_migration/stub.manifest",
        "source\tdocs/long.md\n",
    );
    let quiet = trailmark(&root.path, &["check"]);
    expect(&quiet, 0, "C18 stays off before cutover");
    assert!(!quiet.stdout.contains("C18"), "{}", quiet.stdout);
    replace(
        &root.path.join("trail.toml"),
        "cutover = false",
        "cutover = true",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C18"), "{}", out.stdout);
    assert!(out.stdout.contains("expected the stub"), "{}", out.stdout);
}

#[test]
fn names_stay_out_of_the_tool() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // AT-16 greps tests/ too, so the host's name is assembled here; the harness name is
    // only banned from src/ (AT-13), so it is written plainly.
    let host = format!("{}{}", "zame", "tka");
    let harness = "claude";
    let mut hits = Vec::new();
    for dir in ["src", "templates", "tests"] {
        walk(&root.join(dir), &host, harness, dir == "src", &mut hits);
    }
    assert!(
        hits.is_empty(),
        "name leaked into the tool:\n{}",
        hits.join("\n")
    );
}

#[test]
fn package_is_its_own_workspace() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let out = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        text.contains(&workspace.display().to_string()),
        "metadata did not name this folder as the workspace\n{text}"
    );
}

#[test]
fn generic_block_fits_and_install_dry_run_prints_no_settings_path() {
    let root = fixture();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let block = std::fs::read_to_string(manifest.join("adapters/generic/agents-block.md")).unwrap();
    let tok = block.chars().count().div_ceil(4);
    assert!(tok <= 600, "agents block is {tok} tok4");
    let out = trailmark(&root.path, &["adapter", "install", "generic", "--dry-run"]);
    expect(&out, 0, "generic dry-run");
    assert!(out.stdout.contains("Paste the block"), "{}", out.stdout);
    assert!(!root.path.join("AGENTS.md").exists());
    let skill = "claude-code";
    let installed = trailmark(&root.path, &["adapter", "install", skill, "--dry-run"]);
    expect(&installed, 0, "skill dry-run");
    assert!(
        installed
            .stdout
            .contains("missing settings file is created"),
        "{}",
        installed.stdout
    );
}

#[test]
fn narrows_keeps_both_rules_and_show_includes_the_later_one() {
    let root = fixture();
    let linked = trailmark(
        &root.path,
        &["link", "GUIDE-2.100", "GUIDE-2.1", "--rel", "narrows"],
    );
    expect(&linked, 0, "narrows");
    let older = std::fs::read_to_string(root.path.join("store/rules/2.1.md")).unwrap();
    assert!(older.contains("status: active"), "{older}");
    assert!(older.contains("narrowed_by:"), "{older}");
    let newer = std::fs::read_to_string(root.path.join("store/rules/2.100.md")).unwrap();
    assert!(newer.contains("narrows:"), "{newer}");
    let shown = trailmark(&root.path, &["show", "GUIDE-2.1"]);
    expect(&shown, 0, "show the earlier rule");
    assert!(
        shown.stdout.contains("Narrowed by GUIDE-2.100"),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("A sourdough tray fits on the rack"),
        "{}",
        shown.stdout
    );
    assert!(shown.stdout.contains("still stands"), "{}", shown.stdout);
    let decided = trailmark(
        &root.path,
        &["link", "DEC-0001", "GUIDE-2.2", "--rel", "narrows"],
    );
    expect(&decided, 0, "a decision narrows a rule");
    let shown = trailmark(&root.path, &["show", "GUIDE-2.2"]);
    expect(&shown, 0, "show the narrowed rule");
    assert!(
        shown.stdout.contains("Narrowed by DEC-0001"),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("Keep the shop name on the bag"),
        "{}",
        shown.stdout
    );
    let why = trailmark(&root.path, &["why", "code/note.ts"]);
    expect(&why, 0, "why");
    assert!(
        why.stdout.contains("narrowed by GUIDE-2.100"),
        "{}",
        why.stdout
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let check = trailmark(&root.path, &["check"]);
    expect(&check, 0, "check");
    assert!(!check.stdout.contains("C19 error"), "{}", check.stdout);
}

#[test]
fn either_pair_stays_together_and_a_feature_names_which_it_followed() {
    let root = fixture();
    let linked = trailmark(
        &root.path,
        &["link", "GUIDE-2.1", "GUIDE-2.100", "--rel", "either"],
    );
    expect(&linked, 0, "either");
    let peer = std::fs::read_to_string(root.path.join("store/rules/2.100.md")).unwrap();
    assert!(peer.contains("either:"), "{peer}");
    let bundle = trailmark(&root.path, &["context", "weigh loaf"]);
    expect(&bundle, 0, "context");
    assert!(bundle.stdout.contains("GUIDE-2.1"), "{}", bundle.stdout);
    assert!(bundle.stdout.contains("GUIDE-2.100"), "{}", bundle.stdout);
    assert!(bundle.stdout.contains("Follow either"), "{}", bundle.stdout);
    let followed = trailmark(
        &root.path,
        &["link", "LOOP-BK2", "GUIDE-2.1", "--rel", "follows"],
    );
    expect(&followed, 0, "follows");
    let feature = trailmark(&root.path, &["show", "LOOP-BK2"]);
    expect(&feature, 0, "show the feature");
    assert!(
        feature.stdout.contains("Followed GUIDE-2.1"),
        "{}",
        feature.stdout
    );
    assert!(
        feature.stdout.contains("Also accepted: GUIDE-2.100"),
        "{}",
        feature.stdout
    );
    let earlier = trailmark(&root.path, &["show", "GUIDE-2.1"]);
    expect(&earlier, 0, "show the chosen convention");
    assert!(
        earlier.stdout.contains("Followed by LOOP-BK2"),
        "{}",
        earlier.stdout
    );
    let clash = trailmark(
        &root.path,
        &["link", "GUIDE-2.100", "GUIDE-2.1", "--rel", "narrows"],
    );
    expect(&clash, 6, "either and narrows cannot share a pair");
    assert!(
        clash.stderr.contains("neither one wins"),
        "{}",
        clash.stderr
    );
}

#[test]
fn narrows_rejects_a_tombstone_and_a_cycle() {
    let root = fixture();
    let dead = trailmark(
        &root.path,
        &["link", "GUIDE-2.4", "GUIDE-2.3", "--rel", "narrows"],
    );
    expect(&dead, 6, "tombstone");
    assert!(dead.stderr.contains("superseded"), "{}", dead.stderr);
    assert!(dead.stderr.contains("stay active"), "{}", dead.stderr);
    let first = trailmark(
        &root.path,
        &["link", "GUIDE-2.100", "GUIDE-2.1", "--rel", "narrows"],
    );
    expect(&first, 0, "first narrows");
    let cycle = trailmark(
        &root.path,
        &["link", "GUIDE-2.1", "GUIDE-2.100", "--rel", "narrows"],
    );
    expect(&cycle, 6, "cycle");
    assert!(cycle.stderr.contains("narrows cycle"), "{}", cycle.stderr);
    let older = std::fs::read_to_string(root.path.join("store/rules/2.1.md")).unwrap();
    assert!(!older.contains("\nnarrows:"), "{older}");
}

#[test]
fn an_older_narrower_is_a_warning() {
    let root = fixture();
    let linked = trailmark(
        &root.path,
        &["link", "GUIDE-2.1", "GUIDE-2.100", "--rel", "narrows"],
    );
    expect(&linked, 0, "older narrows newer");
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let check = trailmark(&root.path, &["check"]);
    expect(&check, 0, "warning does not fail check");
    assert!(check.stdout.contains("C19 warning"), "{}", check.stdout);
    assert!(
        check.stdout.contains("before 2026-10-27"),
        "{}",
        check.stdout
    );
}

#[test]
fn six_narrowers_warn_and_show_keeps_the_five_newest() {
    let root = fixture();
    for n in 1..=6 {
        let num = 4000 + n;
        let date = format!("2026-11-{n:02}");
        write(
            &root.path,
            &format!("store/rules/2.{num}.md"),
            &format!(
                "---\nid: GUIDE-2.{num}\ntype: rule\ntitle: Narrower {num}\nshort: Narrower {num}\ndate: {date}\nstatus: active\nlegacy: true\nareas: [web]\nsource: unknown\n---\n### 2.{num} Narrower {num}\n\n**Incident ({date}).** A later sentence.\n\n> **Rule.** Narrower {num} says alpha{num}.\n"
            ),
        );
        let linked = trailmark(
            &root.path,
            &[
                "link",
                &format!("GUIDE-2.{num}"),
                "GUIDE-2.1",
                "--rel",
                "narrows",
            ],
        );
        expect(&linked, 0, "link narrower");
    }
    let shown = trailmark(&root.path, &["show", "GUIDE-2.1"]);
    expect(&shown, 0, "show");
    assert!(
        shown.stdout.contains("showing the 5 newest"),
        "{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("Narrower 4006 says"),
        "{}",
        shown.stdout
    );
    assert!(
        !shown.stdout.contains("Narrower 4001 says"),
        "{}",
        shown.stdout
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let check = trailmark(&root.path, &["check"]);
    expect(&check, 0, "check");
    assert!(check.stdout.contains("6 narrowers"), "{}", check.stdout);
}

#[test]
fn area_index_folds_when_the_budget_is_short() {
    let root = fixture();
    replace(
        &root.path.join("trail.toml"),
        "rules_area_index = 3000",
        "rules_area_index = 70",
    );
    for n in 1..=6 {
        let num = 5000 + n;
        write(
            &root.path,
            &format!("store/rules/2.{num}.md"),
            &format!(
                "---\nid: GUIDE-2.{num}\ntype: rule\ntitle: Area rule {num}\nshort: Area rule {num}\ndate: 2026-10-24\nstatus: active\nlegacy: true\nareas: [web]\nsource: unknown\n---\n### 2.{num} Area rule {num}\n\n**Incident (2026-10-24).** Extra.\n\n> **Rule.** Area rule {num}.\n"
            ),
        );
    }
    // The default: an area index that cannot list every rule fails, writing nothing.
    let refused = trailmark(&root.path, &["index"]);
    expect(&refused, 3, "area index over budget");
    assert!(
        refused.stderr.contains("index/rules-web.md"),
        "{}",
        refused.stderr
    );
    // A host may choose folding; then index says which file it folded.
    replace(
        &root.path.join("trail.toml"),
        "rules_area_index = 70",
        "rules_area_index = 70\nfold_rules_area = true",
    );
    let indexed = trailmark(&root.path, &["index"]);
    expect(&indexed, 0, "folded index");
    assert!(indexed.stdout.contains("folded:"), "{}", indexed.stdout);
    let page = std::fs::read_to_string(root.path.join("store/index/rules-web.md")).unwrap();
    assert!(page.contains("more rules"), "{page}");
    assert!(
        page.contains("trailmark find --type rule --area web"),
        "{page}"
    );
}

#[test]
fn add_suggests_narrows_or_either_when_the_words_overlap() {
    let root = fixture();
    let body = root.path.join("overlap.md");
    std::fs::write(
        &body,
        "**Incident (2026-10-26).** A loaf left the counter light.\n\n> **Rule.** Sourdough loaves are weighed and wrapped.\n",
    )
    .unwrap();
    let added = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Sourdough loaves are weighed and wrapped",
            "--areas",
            "web",
            "--body",
            body.to_str().unwrap(),
        ],
    );
    expect(&added, 0, "add");
    assert!(
        added
            .stdout
            .contains("trailmark link GUIDE-2.101 GUIDE-2.1 --rel narrows"),
        "{}",
        added.stdout
    );
    assert!(
        added
            .stdout
            .contains("trailmark link GUIDE-2.101 GUIDE-2.1 --rel either"),
        "{}",
        added.stdout
    );
}

#[test]
fn a_deploy_number_with_a_hash_resolves() {
    let root = prepared();
    for token in ["#4", "deploy #4"] {
        let out = trailmark(&root.path, &["show", token, "--fields", "id"]);
        expect(&out, 0, token);
        assert!(out.stdout.contains("id: DEP-4"), "{token}: {}", out.stdout);
    }
    replace(
        &root.path.join("store/loops/BK2.md"),
        "## Brief\n",
        "## Brief\nShipped in #4, after deploy #4 was checked.\n",
    );
    let out = trailmark(&root.path, &["index"]);
    expect(&out, 0, "index");
    let out = trailmark(&root.path, &["check"]);
    assert!(!out.stdout.contains("C05"), "{}", out.stdout);
}

#[test]
fn lowercase_words_shaped_like_ids_are_not_citations() {
    let root = prepared();
    write(
        &root.path,
        "code/words.ts",
        "// git rev-parse HEAD; open-sesame; or-id; Dec-31\nconst cls = 'open-editor';\n",
    );
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "check");
    assert!(!out.stdout.contains("C10"), "{}", out.stdout);
}

#[test]
fn add_is_refused_only_for_errors_it_causes() {
    let root = prepared();
    // An error already in the store (a dead link in an old record) is not this write's.
    replace(
        &root.path.join("store/loops/BK2.md"),
        "## Brief\n",
        "## Brief\nSee DEC-0999.\n",
    );
    let ok = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Keep the grid on one line",
            "--areas",
            "web",
        ],
    );
    expect(&ok, 0, "add with an old dead link elsewhere");
    // A new dead link is.
    write(&root.path, "rule.txt", "Follow DEC-0998.\n");
    let rule = root.path.join("rule.txt").display().to_string();
    let bad = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Cites nothing real",
            "--areas",
            "web",
            "--part",
            &format!("rule={rule}"),
        ],
    );
    expect(&bad, 6, "add with a new dead link");
    assert!(bad.stderr.contains("DEC-0998"), "{}", bad.stderr);
    assert!(!bad.stderr.contains("DEC-0999"), "{}", bad.stderr);
}

#[test]
fn add_refuses_a_value_that_would_break_the_frontmatter() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Evil\n---\nbody",
            "--areas",
            "web",
        ],
    );
    expect(&out, 2, "newline in the title");
    let out = trailmark(
        &root.path,
        &[
            "add",
            "decision",
            "--title",
            "Fine",
            "--field",
            "decided_by=author",
            "--field",
            "basis=silence",
            "--field",
            "quote=a\nstatus: revoked",
        ],
    );
    expect(&out, 2, "newline in a field");
    let out = trailmark(
        &root.path,
        &[
            "add",
            "deploy",
            "86",
            "--title",
            "No word",
            "--field",
            "target=web",
            "--field",
            "legacy=true",
        ],
    );
    expect(&out, 2, "legacy set by hand");
    assert!(!root.path.join("store/deploys/86.md").exists());
    assert!(!root.path.join("store/rules/2.101.md").exists());
}

#[test]
fn context_prints_a_rule_once() {
    let root = prepared();
    let out = trailmark(&root.path, &["context", "weigh loaf fixture assert"]);
    expect(&out, 0, "context");
    let text = out.stdout;
    let (full, lines) = match (
        text.find("## Rules — full text"),
        text.find("## Rules — one line each"),
    ) {
        (Some(a), Some(b)) => (&text[a..b], &text[b..]),
        _ => return,
    };
    for line in lines.lines().filter(|l| l.starts_with("GUIDE-")) {
        let id = line.split_whitespace().next().unwrap();
        assert!(
            !full.contains(&format!("{id}  ")),
            "{id} is in both lists\n{text}"
        );
    }
}

#[test]
fn a_review_starts_without_a_verdict_and_owes_one_when_done() {
    let root = prepared();
    let review = trailmark(
        &root.path,
        &[
            "add",
            "review",
            "--for",
            "LOOP-BK2",
            "--title",
            "Review the tray",
        ],
    );
    expect(&review, 0, "add review at launch");
    let open = trailmark(
        &root.path,
        &[
            "add",
            "open",
            "--for",
            "REV-BK2",
            "--title",
            "A risk the review found",
            "--areas",
            "web",
            "--kind",
            "risk",
            "--severity",
            "low",
            "--owner",
            "author",
        ],
    );
    expect(&open, 0, "a review opens an item from its own block");
    replace(
        &root.path.join("store/reviews/BK2.md"),
        "status: running",
        "status: done",
    );
    let out = trailmark(&root.path, &["index"]);
    expect(&out, 0, "index");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 1, "a done review without a verdict");
    assert!(
        out.stdout
            .contains("verdict is required when status is done"),
        "{}",
        out.stdout
    );
}

#[test]
fn the_skill_adapter_never_replaces_an_existing_settings_file() {
    let root = fixture();
    let harness = ".claude".to_string();
    let settings = root.path.join(&harness).join("settings.json");
    write(
        &root.path,
        &format!("{harness}/settings.json"),
        "{\"permissions\":{\"allow\":[]}}\n",
    );
    let skill = format!("{harness}-code");
    let skill = skill.trim_start_matches('.');
    let out = trailmark(&root.path, &["adapter", "install", skill, "--force"]);
    // the hook and the skill are replaced; settings stay. Exit 0, not 7,
    // so a caller can tell "settings left alone" from "nothing was written".
    expect(&out, 0, "an existing settings file");
    assert!(
        root.path
            .join(".claude/hooks/trail-guard-read.sh")
            .is_file(),
        "the hook was written"
    );
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        "{\"permissions\":{\"allow\":[]}}\n"
    );
    assert!(
        out.stderr
            .contains("\"matcher\": \"Read|Grep|Glob|Bash|Edit|Write|MultiEdit\""),
        "{}",
        out.stderr
    );
}

#[test]
fn a_fresh_install_writes_settings_when_the_file_is_absent() {
    let root = fixture();
    let out = trailmark(&root.path, &["adapter", "install", "claude-code"]);
    expect(&out, 0, "install with no settings file");
    let settings = root.path.join(".claude/settings.json");
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("trail-guard-read.sh"), "{text}");
    assert!(root
        .path
        .join(".claude/hooks/trail-guard-read.sh")
        .is_file());
}

#[test]
fn the_default_scheme_mints_rule_n_and_section_style_mints_guide() {
    let root = fixture();
    // The shared fixture is section style. A store with no [ids] mints RULE-1.
    let bare = root.path.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    write(
        &bare,
        "trail.toml",
        "name = \"Bare\"\nrequires = \">=0.1.0\"\nstore = \"docs/records\"\n\n[areas]\napp = { keywords = [\"loaf\"], paths = [\"src/**\"] }\n",
    );
    std::fs::create_dir_all(bare.join("docs/records")).unwrap();
    expect(
        &trailmark(
            &bare,
            &[
                "add",
                "loop",
                "B1",
                "--title",
                "First slice",
                "--item",
                "none",
                "--platform",
                "docs",
            ],
        ),
        0,
        "add loop",
    );
    let rule = trailmark(
        &bare,
        &[
            "add",
            "rule",
            "--for",
            "B1",
            "--title",
            "A loaf is weighed first",
            "--areas",
            "app",
        ],
    );
    expect(&rule, 0, "add rule");
    assert!(rule.stdout.starts_with("RULE-1\n"), "{}", rule.stdout);
}

#[test]
fn show_expands_a_range_of_rules() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &["show", "GUIDE-2.1..GUIDE-2.999", "--fields", "id"],
    );
    expect(&out, 0, "range");
    assert!(out.stdout.contains("id: GUIDE-2.1\n"), "{}", out.stdout);
    assert!(out.stdout.contains("id: GUIDE-2.100\n"), "{}", out.stdout);
}

#[test]
fn parallel_writers_mint_distinct_ids_and_keep_every_reservation() {
    let root = prepared();
    let path = root.path.clone();
    let handles: Vec<_> = (0..6)
        .map(|k| {
            let path = path.clone();
            std::thread::spawn(move || {
                let title = format!("Parallel rule {k}");
                trailmark(
                    &path,
                    &[
                        "add", "rule", "--for", "LOOP-BK2", "--title", &title, "--areas", "web",
                    ],
                )
            })
        })
        .collect();
    let mut ids = Vec::new();
    for h in handles {
        let out = h.join().unwrap();
        expect(&out, 0, "parallel add");
        ids.push(out.stdout.lines().next().unwrap().to_string());
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 6, "{ids:?}");
    let loops: Vec<_> = ["PX1", "PX2"]
        .iter()
        .map(|key| {
            let path = path.clone();
            let key = key.to_string();
            std::thread::spawn(move || {
                trailmark(
                    &path,
                    &[
                        "add",
                        "loop",
                        &key,
                        "--title",
                        "Parallel loop",
                        "--item",
                        "none",
                        "--platform",
                        "web",
                    ],
                )
            })
        })
        .collect();
    for h in loops {
        expect(&h.join().unwrap(), 0, "parallel loop");
    }
    let reserved = reservations_text(&root.path.join("store"));
    assert!(
        reserved.contains("LOOP-PX1") && reserved.contains("LOOP-PX2"),
        "{reserved}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(root.path.join("store"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".trailtmp") || n == ".trail.lock")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_malformed_reservations_file_is_an_error_not_an_empty_one() {
    let root = prepared();
    let path = root.path.join("store/reservations.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("oops [[\n");
    std::fs::write(&path, text).unwrap();
    let out = trailmark(
        &root.path,
        &[
            "add", "rule", "--for", "LOOP-BK2", "--title", "x", "--areas", "web",
        ],
    );
    expect(&out, 4, "malformed reservations");
    assert!(out.stderr.contains("reservations.toml"), "{}", out.stderr);
}

#[test]
fn a_held_lock_refuses_the_writer() {
    let root = prepared();
    write(&root.path, "store/.trail.lock", "pid 1\n");
    // The wait is shortened for this process only, so other tests keep the normal wait.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_trailmark"))
        .env("TRAIL_LOCK_WAIT_MS", "100")
        .arg("--root")
        .arg(&root.path)
        .args([
            "add", "rule", "--for", "LOOP-BK2", "--title", "x", "--areas", "web",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains(".trail.lock"));
    // A reader does not need the lock.
    expect(
        &trailmark(&root.path, &["show", "GUIDE-2.1"]),
        0,
        "show while locked",
    );
}

#[test]
fn unknown_flags_and_unknown_config_keys_fail_loudly() {
    let root = prepared();
    let out = trailmark(&root.path, &["find", "layout", "--limt", "3"]);
    expect(&out, 2, "unknown flag");
    assert!(out.stderr.contains("--limit"), "{}", out.stderr);
    replace(
        &root.path.join("trail.toml"),
        "[reserve]",
        "[budgts]\ncore = 1\n\n[reserve]",
    );
    let out = trailmark(&root.path, &["show", "GUIDE-2.1"]);
    expect(&out, 4, "unknown config key");
    assert!(
        out.stderr.contains("did you mean 'budgets'"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_local_overlay_supplies_machine_settings() {
    let root = prepared();
    write(&root.path, ".trail/local.toml", "name = \"Overlay\"\n");
    let out = trailmark(&root.path, &["context", "--core"]);
    expect(&out, 0, "core");
    assert!(
        out.stdout.contains("# Start here — Overlay"),
        "{}",
        out.stdout
    );
}

#[test]
fn find_searches_one_part_and_reports_no_hits() {
    let root = prepared();
    let out = trailmark(&root.path, &["find", "oven", "--part", "not-verified"]);
    expect(&out, 0, "find --part");
    assert!(out.stdout.contains("LOOP-"), "{}", out.stdout);
    let none = trailmark(&root.path, &["find", "zzqqxx"]);
    expect(&none, 1, "no hits");
}

#[test]
fn measure_reads_a_tasks_file() {
    let root = prepared();
    write(
        &root.path,
        "tasks.toml",
        "[[task]]\nname = \"layout pass\"\ntext = \"layout translation\"\nareas = [\"web\"]\n",
    );
    let file = root.path.join("tasks.toml").display().to_string();
    let out = trailmark(&root.path, &["measure", "--tasks", &file]);
    expect(&out, 0, "measure");
    assert!(out.stdout.contains("context layout pass"), "{}", out.stdout);
}

#[test]
fn context_says_what_it_left_out_and_explains_in_markdown() {
    let root = prepared();
    let out = trailmark(&root.path, &["context", "weigh loaf", "--explain"]);
    expect(&out, 0, "context");
    assert!(out.stdout.contains("## Not included"), "{}", out.stdout);
    assert!(out.stdout.contains("## Why these rules"), "{}", out.stdout);
    assert!(out.stdout.contains("words "), "{}", out.stdout);
}

#[test]
fn context_carries_the_fields_founding_rule_and_only_the_facts_that_match() {
    let root = prepared();
    write(
        &root.path,
        "store/facts/devices.md",
        "---\nid: FACT-devices\ntype: fact\ntitle: Two spare bowls\ndate: 2026-10-24\nstatus: active\nkind: device\nscope: repo\n---\nA log of the spare bowls.\n\n**Why:** the shelf is shared.\n\n**How to apply:** count them before the close.\n",
    );
    write(
        &root.path,
        "store/facts/longest.md",
        "---\nid: FACT-longest\ntype: fact\ntitle: The longest loaf name sets the tray\ndate: 2026-10-24\nstatus: active\nkind: trap\nscope: repo\n---\nThe tray on the rack is set by the longest loaf name on the board.\n\n**Why:** measured.\n\n**How to apply:** test the tray on the board first.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(
        &root.path,
        &["context", "the tray on the rack overflows on the board"],
    );
    expect(&out, 0, "context");
    let text = out.stdout;
    let full = text.split("## Rules — full text").nth(1).unwrap_or("");
    assert!(full.contains("GUIDE-2.100  "), "{text}");
    assert!(text.contains("FACT-longest"), "{text}");
    assert!(!text.contains("FACT-devices"), "{text}");
}

#[test]
fn the_core_lists_a_short_fixed_number_of_cited_rules() {
    let root = prepared();
    replace(
        &root.path.join("trail.toml"),
        "[reserve]",
        "[budgets.extra]\n[reserve]",
    );
    // An unknown nested key is refused too.
    expect(
        &trailmark(&root.path, &["show", "GUIDE-2.1"]),
        4,
        "budgets.extra",
    );
    replace(&root.path.join("trail.toml"), "[budgets.extra]\n", "");
    replace(
        &root.path.join("trail.toml"),
        "items_index = 4000",
        "items_index = 4000\ncore_hot_rules = 1",
    );
    let out = trailmark(&root.path, &["context", "--core"]);
    expect(&out, 0, "core");
    let hot = out.stdout.split("## Most-cited rules").nth(1).unwrap_or("");
    assert_eq!(
        hot.lines().filter(|l| l.starts_with("GUIDE-")).count(),
        1,
        "{}",
        out.stdout
    );
}

#[test]
fn the_adapter_shows_a_diff_and_replaces_only_with_force() {
    let root = prepared();
    expect(
        &trailmark(&root.path, &["adapter", "install", "generic"]),
        0,
        "first install",
    );
    write(&root.path, ".trail/agents-block.md", "# Edited by hand\n");
    let out = trailmark(&root.path, &["adapter", "install", "generic"]);
    expect(&out, 7, "differs");
    assert!(
        out.stderr.contains("  - # Edited by hand"),
        "{}",
        out.stderr
    );
    expect(
        &trailmark(&root.path, &["adapter", "install", "generic", "--force"]),
        0,
        "force",
    );
    let text = std::fs::read_to_string(root.path.join(".trail/agents-block.md")).unwrap();
    assert!(!text.contains("Edited by hand"), "{text}");
}

#[test]
fn a_new_rule_has_no_heading_and_a_new_deploy_supersedes_the_live_one() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "LOOP-BK2",
            "--title",
            "Plain body",
            "--areas",
            "web",
        ],
    );
    expect(&out, 0, "add rule");
    let text = std::fs::read_to_string(root.path.join("store/rules/2.101.md")).unwrap();
    assert!(!text.contains("### "), "{text}");
    let deploy = trailmark(
        &root.path,
        &[
            "add",
            "deploy",
            "86",
            "--title",
            "Next",
            "--field",
            "target=web",
            "--field",
            "time=2026-10-26T12:00Z",
            "--field",
            "authorized_by=author",
            "--field",
            "word=ship it",
            "--field",
            "word_date=2026-10-26",
            "--field",
            "backup=none",
            "--field",
            "backup_word=no backup",
            "--field",
            "commit=abcdef1",
            "--field",
            "binary_sha256=bb",
            "--field",
            "migrations=76/76",
            "--field",
            "carries=LOOP-BK1",
            "--field",
            "rollback=road 1: cp server.85 server",
        ],
    );
    expect(&deploy, 0, "deploy");
    let old = std::fs::read_to_string(root.path.join("store/deploys/4.md")).unwrap();
    assert!(old.contains("status: superseded"), "{old}");
    let new = std::fs::read_to_string(root.path.join("store/deploys/86.md")).unwrap();
    assert!(new.contains("previous: DEP-4"), "{new}");
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let core = trailmark(&root.path, &["context", "--core"]);
    assert!(core.stdout.contains("Production DEP-86"), "{}", core.stdout);
    // Two live deploys of one target are refused by the checker.
    replace(
        &root.path.join("store/deploys/4.md"),
        "status: superseded",
        "status: live",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("are all live for target web"),
        "{}",
        out.stdout
    );
}

#[test]
fn index_writes_the_start_file_for_a_harness_memory() {
    let root = prepared();
    let text = std::fs::read_to_string(root.path.join("store/index/start.md")).unwrap();
    assert!(text.contains("trailmark context"), "{text}");
    assert!(text.chars().count().div_ceil(4) <= 400, "{text}");
}

#[test]
fn legacy_informal_refs_follow_the_hosts_policy() {
    let root = prepared();
    replace(
        &root.path.join("store/loops/BK1.md"),
        "## Brief\n",
        "## Brief\nShipped after #99 and deploy #98.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    replace(
        &root.path.join("store/loops/BK1.md"),
        "status: done",
        "status: done\nlegacy: true",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let strict = trailmark(&root.path, &["check"]);
    assert!(strict.stdout.contains("dead link #99"), "{}", strict.stdout);
    replace(
        &root.path.join("trail.toml"),
        "[reserve]",
        "[check]\nlegacy_context_refs = \"skip\"\n\n[reserve]",
    );
    let skipped = trailmark(&root.path, &["check"]);
    assert!(!skipped.stdout.contains("#99"), "{}", skipped.stdout);
}

#[test]
fn figures_must_exist_and_travel_with_a_rule() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &[
            "figure",
            "GUIDE-2.1",
            "--add",
            "figures/rye-board.png | Rye label overflows the board",
        ],
    );
    expect(&out, 6, "missing picture");
    assert!(out.stderr.contains("C20"), "{}", out.stderr);
    assert!(
        out.stderr.contains("relative to the store")
            && out.stderr.contains("store/figures/rye-board.png"),
        "{}",
        out.stderr
    );
    write(
        &root.path,
        "store/figures/rye-board.png",
        "not really a png",
    );
    let out = trailmark(
        &root.path,
        &[
            "figure",
            "GUIDE-2.1",
            "--add",
            "figures/rye-board.png | Rye label overflows the board",
        ],
    );
    expect(&out, 0, "figure added");
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let show = trailmark(&root.path, &["show", "GUIDE-2.1"]);
    assert!(
        show.stdout
            .contains("Figure: store/figures/rye-board.png — Rye label overflows the board"),
        "{}",
        show.stdout
    );
    let ctx = trailmark(&root.path, &["context", "weigh loaf"]);
    assert!(
        ctx.stdout.contains("Figure: store/figures/rye-board.png"),
        "{}",
        ctx.stdout
    );
    let out = trailmark(
        &root.path,
        &["figure", "GUIDE-2.1", "--add", "figures/rye-board.png"],
    );
    expect(&out, 6, "no caption");
    let out = trailmark(
        &root.path,
        &["figure", "GUIDE-2.1", "--add", "../outside.png | x"],
    );
    expect(&out, 6, "outside the store");
}

#[test]
fn a_symlinked_directory_is_not_scanned_twice() {
    let root = prepared();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path.join("code"), root.path.join("code/again")).unwrap();
        let out = trailmark(&root.path, &["check"]);
        assert!(out.stdout.contains(" 1 code reference"), "{}", out.stdout);
    }
}

#[test]
fn the_harness_memory_index_is_replaced_only_with_force_and_kept_beside() {
    let root = prepared();
    let memory = root.path.join("mem/MEMORY.md");
    write(&root.path, "mem/MEMORY.md", "# old index\n");
    write(
        &root.path,
        ".trail/local.toml",
        &format!(
            "[adapters.claude_code]\nmemory_md = \"{}\"\n",
            memory.display()
        ),
    );
    expect(
        &trailmark(&root.path, &["adapter", "install", "claude-code"]),
        0,
        "without force",
    );
    assert_eq!(std::fs::read_to_string(&memory).unwrap(), "# old index\n");
    expect(
        &trailmark(
            &root.path,
            &["adapter", "install", "claude-code", "--force"],
        ),
        0,
        "force",
    );
    assert!(std::fs::read_to_string(&memory)
        .unwrap()
        .contains("trailmark context"));
    let kept = root.path.join("mem/MEMORY.pre-trailmark.md");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "# old index\n");
}

fn walk(dir: &Path, host: &str, harness: &str, src: bool, hits: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, host, harness, src, hits);
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.ends_with(".rs") || name.ends_with(".md") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let lower = text.to_lowercase();
            // A name assembled from fragments is the same leak; `"cla"` is how it was hidden.
            let assembled = src && text.contains("\"cla\"");
            if lower.contains(&host.to_lowercase()) || (src && lower.contains(harness)) || assembled
            {
                hits.push(path.display().to_string());
            }
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Dev")
        .env("GIT_AUTHOR_EMAIL", "dev@example.com")
        .env("GIT_COMMITTER_NAME", "Dev")
        .env("GIT_COMMITTER_EMAIL", "dev@example.com")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

///  help answers before a `trail.toml` exists; a loop, review or item is named by its
/// short key; a review's title defaults to "Review of <KEY>".
#[test]
fn short_keys_help_and_a_default_review_title() {
    let help = trailmark::run(&["--help".to_string()], "");
    expect(&help, 0, "--help without a store");
    assert!(help.stdout.contains("Commands:"), "{}", help.stdout);
    let version = trailmark::run(&["--version".to_string()], "");
    expect(&version, 0, "--version");
    assert!(
        version.stdout.starts_with("trailmark "),
        "{}",
        version.stdout
    );
    let flags = trailmark::run(&["help".to_string(), "add".to_string()], "");
    expect(&flags, 0, "help add");
    assert!(flags.stdout.contains("--for"), "{}", flags.stdout);

    let root = fixture();
    let full = trailmark(&root.path, &["context", "--loop", "LOOP-BK2"]);
    expect(&full, 0, "context --loop LOOP-BK2");
    let short = trailmark(&root.path, &["context", "--loop", "BK2"]);
    expect(&short, 0, "context --loop BK2");
    assert_eq!(full.stdout, short.stdout);

    let rule = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "BK2",
            "--title",
            "Short keys mint",
            "--areas",
            "web",
        ],
    );
    expect(&rule, 0, "add rule --for BK2");
    assert!(rule.stdout.contains("GUIDE-2.101"), "{}", rule.stdout);

    let review = trailmark(&root.path, &["add", "review", "--for", "BK2"]);
    expect(&review, 0, "add review --for BK2, no title");
    let text = std::fs::read_to_string(root.path.join("store/reviews/BK2.md")).unwrap();
    assert!(text.contains("title: Review of BK2"), "{text}");
    let by_short = trailmark(&root.path, &["context", "--review", "BK2"]);
    expect(&by_short, 0, "context --review BK2");
    assert!(by_short.stdout.contains("REV-BK2"), "{}", by_short.stdout);
}

///  a deploy migrated from prose has a date and no commit, migrations or rollback; the
/// "Now" line prints what it has, never an empty field (`a date and an empty field`).
#[test]
fn the_now_line_prints_only_the_fields_a_deploy_has() {
    let root = fixture();
    let dep = root.path.join("store/deploys/4.md");
    replace(&dep, "status: live\n", "status: live\nlegacy: true\n");
    for line in [
        "commit: abcdef01\n",
        "migrations: none\n",
        "rollback: [\"put the previous binary back\"]\n",
    ] {
        replace(&dep, line, "");
    }
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let core = std::fs::read_to_string(root.path.join("store/index/core.md")).unwrap();
    assert!(core.contains("Production DEP-4 (2026-10-24)."), "{core}");
    let deploys = std::fs::read_to_string(root.path.join("store/index/deploys.md")).unwrap();
    assert!(!deploys.contains("rollback: \n"), "{deploys}");
    assert!(!deploys.contains("backup  "), "{deploys}");
}

///  the step that supersedes another may name it ("Supersedes STEP-2") without a C15
/// warning; the review bundle carries the step that superseded STEP-4.
#[test]
fn a_superseding_step_names_the_old_one_and_the_review_bundle_follows_it() {
    let root = fixture();
    write(
        &root.path,
        "body.md",
        "1. **Brief.** Restate the ask, then `trailmark context`. Supersedes STEP-2.\n",
    );
    let body = root.path.join("body.md").display().to_string();
    let add = trailmark(
        &root.path,
        &[
            "add", "charter", "STEP-6", "--title", "Brief", "--body", &body,
        ],
    );
    expect(&add, 0, "add charter STEP-6");
    expect(
        &trailmark(
            &root.path,
            &["link", "STEP-6", "STEP-2", "--rel", "supersedes"],
        ),
        0,
        "link supersedes",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let check = trailmark(&root.path, &["check"]);
    assert!(!check.stdout.contains("STEP-6.md:"), "{}", check.stdout);
    let core = std::fs::read_to_string(root.path.join("store/index/core.md")).unwrap();
    assert!(core.contains("STEP-6  Brief."), "{core}");
    assert!(!core.contains("STEP-2  "), "{core}");

    write(
        &root.path,
        "review.md",
        "5. **Review.** Start with `trailmark context --review <KEY>`.\n",
    );
    let body = root.path.join("review.md").display().to_string();
    expect(
        &trailmark(
            &root.path,
            &[
                "add", "charter", "STEP-8", "--title", "Review", "--body", &body,
            ],
        ),
        0,
        "add charter STEP-8",
    );
    expect(
        &trailmark(
            &root.path,
            &["link", "STEP-8", "STEP-4", "--rel", "supersedes"],
        ),
        0,
        "link STEP-8",
    );
    expect(
        &trailmark(&root.path, &["add", "review", "--for", "BK2"]),
        0,
        "add review",
    );
    let bundle = trailmark(&root.path, &["context", "--review", "BK2"]);
    expect(&bundle, 0, "context --review");
    assert!(
        bundle.stdout.contains("STEP-8  Review"),
        "{}",
        bundle.stdout
    );
}

///  a target record longer than its share of the bundle is cut, not dropped.
#[test]
fn a_long_target_is_cut_not_dropped() {
    let root = fixture();
    let long = "A long line about the morning bake and what it touched.\n".repeat(400);
    replace(
        &root.path.join("store/loops/BK2.md"),
        "One tray has to fit the rack, including the longest loaf name.\n",
        &long,
    );
    let out = trailmark(
        &root.path,
        &["context", "--loop", "BK2", "--format", "json"],
    );
    expect(&out, 0, "context");
    assert!(out.stdout.contains("\"target LOOP-BK2\""), "{}", out.stdout);
    assert!(out.stdout.contains("cut to fit"), "{}", out.stdout);
}

/// Two worktrees of one repository each hold their own `reservations.toml` and their own
/// store lock; before both took the block after what their branch knew, and minted the
/// same numbers. The ledger under the git common directory is shared, under its own lock.
#[test]
fn worktrees_reserving_at_once_take_distinct_blocks() {
    let root = prepared();
    git(&root.path, &["init", "-q"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    let trees: Vec<Root> = (1..=2)
        .map(|k| {
            let path = root.path.with_file_name(format!(
                "{}-wt{k}",
                root.path.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_dir_all(&path);
            git(
                &root.path,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("wt{k}"),
                    &path.display().to_string(),
                ],
            );
            Root { path }
        })
        .collect();
    // Real processes, three loops per worktree, all started together.
    let mut children = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        for n in 0..3 {
            let key = format!("W{}{n}", k + 1);
            let child = std::process::Command::new(env!("CARGO_BIN_EXE_trailmark"))
                .current_dir(&tree.path)
                .args([
                    "add",
                    "loop",
                    &key,
                    "--title",
                    "Parallel loop",
                    "--item",
                    "none",
                    "--platform",
                    "web",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            children.push(child);
        }
    }
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut blocks: Vec<(String, u32, u32)> = Vec::new();
    for tree in &trees {
        let text = reservations_text(&tree.path.join("store"));
        let v: toml::Value = toml::from_str(&text).unwrap();
        for r in v["reservation"].as_array().unwrap() {
            let for_id = r["for"].as_str().unwrap().to_string();
            if r["type"].as_str() == Some("rule") && for_id.starts_with("LOOP-W") {
                let from = r["from"].as_integer().unwrap() as u32;
                let to = r["to"].as_integer().unwrap() as u32;
                blocks.push((for_id, from, to));
            }
        }
    }
    assert_eq!(blocks.len(), 6, "{blocks:?}");
    for (i, a) in blocks.iter().enumerate() {
        for b in &blocks[i + 1..] {
            assert!(a.2 < b.1 || b.2 < a.1, "{a:?} overlaps {b:?}");
        }
    }
    let ledger = std::fs::read_to_string(root.path.join(".git/trail/store/reservations.toml"))
        .expect("the ledger lives in the git common dir");
    for (for_id, _, _) in &blocks {
        assert!(
            ledger.contains(for_id.as_str()),
            "{for_id} missing:\n{ledger}"
        );
    }
    assert!(ledger.contains("worktree = "), "{ledger}");
    let out = trailmark(&trees[0].path, &["check"]);
    assert!(!out.stdout.contains("C21"), "{}", out.stdout);
    // A block another loop took in another worktree is a warning in this one.
    let mut text = ledger.clone();
    text.push_str(
        "[[reservation]]\ntype = \"rule\"\nfrom = 100\nto = 102\nfor = \"LOOP-ZZ9\"\ndate = \"2026-10-27\"\nworktree = \"/elsewhere\"\n\n",
    );
    std::fs::write(root.path.join(".git/trail/store/reservations.toml"), text).unwrap();
    let out = trailmark(&trees[0].path, &["check"]);
    assert!(out.stdout.contains("C21"), "{}", out.stdout);
    assert!(out.stdout.contains("/elsewhere"), "{}", out.stdout);
    for tree in &trees {
        git(
            &root.path,
            &[
                "worktree",
                "remove",
                "--force",
                &tree.path.display().to_string(),
            ],
        );
    }
}

/// A task ended in "…, deploy now line": the one word named the deploy area, and the
/// area's bonus put deploy rules over the task's topic. An area is kept when the rest of the
/// task leads into it, or when its words are a fair share of the task; a keyword is a word,
/// not a substring.
#[test]
fn a_domain_word_in_passing_does_not_bring_its_area() {
    let root = prepared();
    for (n, title) in [
        (11, "A rollback is rehearsed on the release binary"),
        (12, "A rollback road names the migration it undoes"),
        (13, "A rollback after a backup restores the old tables"),
    ] {
        write(
            &root.path,
            &format!("store/rules/2.{n}.md"),
            &format!("---\nid: GUIDE-2.{n}\ntype: rule\ntitle: {title}\nshort: {title}\ndate: 2026-10-24\nstatus: active\nlegacy: true\nareas: [deploy]\nsource: LOOP-BK1\n---\n### 2.{n} {title}\n\n> **Rule.** {title}.\n"),
        );
    }
    write(
        &root.path,
        "store/rules/2.5.md",
        "---\nid: GUIDE-2.5\ntype: rule\ntitle: A cold reader of the readme checks the first bake\nshort: A cold reader of the readme checks the first bake\ndate: 2026-10-24\nstatus: active\nlegacy: true\nareas: [process]\nsource: LOOP-BK1\n---\n### 2.5 Readme\n\n> **Rule.** A cold reader of the readme checks the first bake.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(
        &root.path,
        &[
            "context",
            "a cold reader of the readme, short keys, the rollback line",
            "--explain",
        ],
    );
    expect(&out, 0, "context");
    assert!(
        out.stdout
            .contains("area deploy: 1 of the task's words name it")
            && out.stdout.contains("; dropped"),
        "{}",
        out.stdout
    );
    let why = out.stdout.split("## Why these rules").nth(1).unwrap_or("");
    let first = why.lines().find(|l| l.starts_with("rule ")).unwrap_or("");
    assert!(first.contains("GUIDE-2.5"), "{why}");
    // A task about the area keeps it.
    let out = trailmark(
        &root.path,
        &["context", "rollback after a backup", "--explain"],
    );
    assert!(
        out.stdout.contains("area deploy:") && out.stdout.contains("; kept"),
        "{}",
        out.stdout
    );
    // "token" is not in "tokenizer".
    let out = trailmark(
        &root.path,
        &["context", "the layout tokenizer", "--explain"],
    );
    assert!(!out.stdout.contains("area security"), "{}", out.stdout);
}

/// A cold read: in a new store of four rules every word is held by a quarter of them,
/// and dropping "common" words left only the task's words no rule holds — the bundle had
/// no rules at all. A small store keeps every word.
#[test]
fn a_small_store_keeps_every_word_of_the_task() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &["context", "sourdough loaves are weighed and wrapped"],
    );
    expect(&out, 0, "context");
    let full = out
        .stdout
        .split("## Rules — full text")
        .nth(1)
        .unwrap_or("");
    assert!(full.contains("GUIDE-2.1  "), "{}", out.stdout);
}

/// A loop or a review is closed by the tool: the read guard keeps an agent out of the record
/// file, and the cold reader of could not finish its loop report by editing it.
#[test]
fn close_writes_the_end_of_a_loop_and_of_a_review() {
    let root = prepared();
    expect(
        &trailmark(
            &root.path,
            &[
                "add",
                "loop",
                "CL1",
                "--title",
                "Close me",
                "--item",
                "none",
                "--platform",
                "docs",
            ],
        ),
        0,
        "add loop",
    );
    let part = |name: &str, text: &str| {
        let path = root.path.join(format!("{name}.txt"));
        std::fs::write(&path, text).unwrap();
        format!("{name}={}", path.display())
    };
    let brief = part("brief", "Close a loop with the tool.");
    // No gates: refused, nothing written.
    let out = trailmark(
        &root.path,
        &["close", "CL1", "--review", "owed", "--part", &brief],
    );
    expect(&out, 6, "no gates");
    assert!(out.stderr.contains("--gates"), "{}", out.stderr);
    // Parts left Pending.: refused, and named.
    let out = trailmark(
        &root.path,
        &[
            "close",
            "CL1",
            "--review",
            "owed",
            "--gates",
            "cargo test 3/0",
            "--part",
            &brief,
        ],
    );
    expect(&out, 6, "pending parts");
    assert!(
        out.stderr.contains("built") && out.stderr.contains("not-verified"),
        "{}",
        out.stderr
    );
    let shown = trailmark(&root.path, &["show", "LOOP-CL1"]);
    assert!(shown.stdout.contains("status: running"), "{}", shown.stdout);
    // A part the record does not have.
    let out = trailmark(
        &root.path,
        &[
            "close",
            "CL1",
            "--review",
            "owed",
            "--gates",
            "x",
            "--part",
            &part("findings", "x"),
        ],
    );
    expect(&out, 2, "unknown part");
    let mut args = vec![
        "close".to_string(),
        "CL1".into(),
        "--review".into(),
        "owed".into(),
        "--gates".into(),
        "cargo test 3/0".into(),
        "--gates".into(),
        "clippy clean, -D warnings".into(),
    ];
    for (name, text) in [
        ("brief", "Close a loop with the tool."),
        ("built", "trailmark close."),
        ("verified", "This test."),
        ("gates", "cargo test 3/0; clippy clean."),
        ("not-verified", "Nothing else."),
        ("lessons", "None."),
    ] {
        args.push("--part".into());
        args.push(part(name, text));
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    expect(&trailmark(&root.path, &refs), 0, "close loop");
    let shown = trailmark(&root.path, &["show", "LOOP-CL1"]);
    assert!(shown.stdout.contains("status: done"), "{}", shown.stdout);
    assert!(
        shown
            .stdout
            .contains("gates: [cargo test 3/0, \"clippy clean, -D warnings\"]"),
        "{}",
        shown.stdout
    );
    assert!(shown.stdout.contains("review: owed"), "{}", shown.stdout);
    assert!(!shown.stdout.contains("Pending."), "{}", shown.stdout);
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
    // A review closes with a verdict.
    expect(
        &trailmark(&root.path, &["add", "review", "--for", "CL1"]),
        0,
        "add review",
    );
    let mut args = vec![
        "close".to_string(),
        "REV-CL1".into(),
        "--gates".into(),
        "read the diff".into(),
    ];
    for (name, text) in [
        ("verdict", "Go."),
        ("findings", "None."),
        ("read-and-run-no-change", "The diff."),
        ("gates", "read the diff."),
        ("not-verified", "Nothing."),
    ] {
        args.push("--part".into());
        args.push(part(name, text));
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = trailmark(&root.path, &refs);
    expect(&out, 6, "a review needs a verdict");
    let mut refs = refs.clone();
    refs.extend(["--verdict", "go"]);
    expect(&trailmark(&root.path, &refs), 0, "close review");
    let shown = trailmark(&root.path, &["show", "REV-CL1"]);
    assert!(shown.stdout.contains("verdict: go"), "{}", shown.stdout);
    // A bare key in `show` names the records that have it.
    let out = trailmark(&root.path, &["show", "CL1"]);
    expect(&out, 2, "bare key");
    assert!(out.stderr.contains("LOOP-CL1, REV-CL1"), "{}", out.stderr);
}

/// Split a help example the way a shell would: words, and "double-quoted" words.
fn shell_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' if !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            _ => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

///  `trailmark add <type> --help` names what a type requires, with an example. Every example
/// runs in a fresh store, writes no `Pending.` part, and is refused when any required flag or
/// field it shows is dropped — so the help cannot drift from the checker (was refused four
/// times in turn by `add open`, each refusal naming one thing).
#[test]
fn add_help_examples_run_and_name_every_required_field() {
    let overview = trailmark::run(&["help".to_string(), "add".to_string()], "");
    expect(&overview, 0, "help add");
    let types: Vec<String> = overview
        .stdout
        .split("Types:\n")
        .nth(1)
        .unwrap_or("")
        .lines()
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    assert_eq!(types.len(), 11, "{}", overview.stdout);
    let help = |t: &str| {
        let out = trailmark::run(
            &["add".to_string(), t.to_string(), "--help".to_string()],
            "",
        );
        expect(&out, 0, t);
        out.stdout
    };
    let open = help("open");
    for need in [
        "--kind",
        "--severity",
        "--owner",
        "--areas",
        "--part what=",
        "--part how-to-close=",
        "risk | follow-up | owed-step | unverified | bug",
        "Example:",
    ] {
        assert!(open.contains(need), "open help lacks {need}:\n{open}");
    }
    assert_eq!(
        help("open"),
        trailmark::run(
            &["help".to_string(), "add".to_string(), "open".to_string()],
            ""
        )
        .stdout
    );

    let run_example = |root: &Path, words: &[String]| {
        // Part and body files are written where the example names them, under the root.
        let mut args: Vec<String> = Vec::new();
        for w in &words[1..] {
            if let Some((name, file)) = w.split_once('=') {
                if file.ends_with(".md") && !file.contains('/') {
                    let path = root.join(file);
                    std::fs::write(&path, format!("The {name} of the example.\n")).unwrap();
                    args.push(format!("{name}={}", path.display()));
                    continue;
                }
            }
            if w.ends_with(".md") && !w.contains('=') {
                let path = root.join(w);
                std::fs::write(&path, "A paragraph of the example.\n").unwrap();
                args.push(path.display().to_string());
                continue;
            }
            args.push(w.clone());
        }
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        trailmark(root, &refs)
    };
    for t in &types {
        let text = help(t);
        let example = text
            .split("Example:\n  ")
            .nth(1)
            .and_then(|r| r.lines().next())
            .unwrap_or_else(|| panic!("{t}: no example\n{text}"));
        let words = shell_words(example);
        assert_eq!(&words[..3], &["trailmark", "add", t.as_str()], "{example}");
        let fresh = || {
            let root = prepared();
            expect(
                &trailmark(
                    &root.path,
                    &[
                        "add",
                        "loop",
                        "P1",
                        "--title",
                        "Example loop",
                        "--item",
                        "none",
                        "--platform",
                        "web",
                    ],
                ),
                0,
                "add loop P1",
            );
            root
        };
        let root = fresh();
        let out = run_example(&root.path, &words);
        expect(&out, 0, &format!("{t} example: {example}"));
        let id = out.stdout.lines().next().unwrap_or("").to_string();
        let shown = trailmark(&root.path, &["show", &id]);
        // A loop's and a review's parts are written by `trailmark close`.
        if !matches!(t.as_str(), "loop" | "review") {
            assert!(!shown.stdout.contains("Pending."), "{t}: {}", shown.stdout);
        }
        // Each required flag the example shows, dropped: refused.
        let required: Vec<String> = text
            .split("record is not edited after it is added, so write every part now:\n")
            .nth(1)
            .unwrap_or("")
            .lines()
            .take_while(|l| l.starts_with("  "))
            .map(|l| l.trim().split("  ").next().unwrap_or("").to_string())
            // A part or a body left out is written as `Pending.`, not refused (the help says so).
            .filter(|f| f.starts_with("--") && !f.starts_with("--part") && !f.starts_with("--body"))
            .collect();
        assert!(!required.is_empty(), "{t}: no required rows\n{text}");
        for flag in required {
            let mut head = flag.split_whitespace();
            let name = head.next().unwrap_or("");
            let key = if name == "--field" {
                head.next()
                    .and_then(|kv| kv.split('=').next())
                    .map(|k| format!("{k}="))
            } else {
                None
            };
            let at = (3..words.len()).find(|&i| {
                words[i] == name
                    && match &key {
                        Some(k) => words
                            .get(i + 1)
                            .map(|v| v.starts_with(k.as_str()))
                            .unwrap_or(false),
                        None => true,
                    }
            });
            // `--title` of a review has a default; every other required flag is in the example.
            let Some(at) = at else {
                assert!(
                    t == "review" || name == "--title",
                    "{t}: the example lacks {flag}"
                );
                continue;
            };
            let mut dropped = words.clone();
            dropped.drain(at..at + 2);
            let root = fresh();
            let out = run_example(&root.path, &dropped);
            assert_ne!(
                out.code, 0,
                "{t} without {flag} was accepted: {}",
                out.stdout
            );
        }
    }
}

///  the Claude Code read guard lets a picture through (a contact sheet is a PNG of
/// hundreds of kilobytes, and a record's figure lives under the store), and still stops a
/// large text file and a store record.
#[test]
fn read_guard_passes_pictures_and_stops_large_text() {
    let root = fixture();
    let hooks = root.path.join(".claude/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("guard.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/claude-code/hooks/guard-read.sh"),
        &script,
    )
    .unwrap();
    std::fs::write(hooks.join("never-whole.txt"), "store/\n").unwrap();
    let mut picture: Vec<u8> = b"\x89PNG\r\n\x1a\n".to_vec();
    picture.extend((0..300_000u32).map(|i| (i % 251) as u8));
    for name in ["sheet.png", "frame.JPG", "store/figures/fig.webp"] {
        let path = root.path.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &picture).unwrap();
    }
    std::fs::write(root.path.join("big.txt"), "abcdefghi\n".repeat(20_000)).unwrap();
    let guard = |rel: &str| {
        use std::io::Write;
        let input = format!(
            "{{\"tool_name\":\"Read\",\"tool_input\":{{\"file_path\":\"{}\"}}}}",
            root.path.join(rel).display()
        );
        let mut child = std::process::Command::new("sh")
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap_or(-1)
    };
    assert_eq!(guard("sheet.png"), 0);
    assert_eq!(guard("frame.JPG"), 0);
    assert_eq!(guard("store/figures/fig.webp"), 0);
    assert_eq!(guard("big.txt"), 2);
    assert_eq!(guard("store/rules/2.1.md"), 2);
}

///  the guard sees the other ways around trailmark. Loops grepped the store from Bash and
/// edited records with `sed -i`; those run and are logged (in a git repository, to
/// `<common dir>/trail/reads.tsv`), and after the tool the agent is told the command and the
/// miss. A removed document's git history is blocked unless the command says its brief asked.
#[test]
fn read_guard_logs_shell_reads_and_closes_removed_history() {
    let root = fixture();
    git(&root.path, &["init", "-q"]);
    let hooks = root.path.join(".claude/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("guard.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/claude-code/hooks/guard-read.sh"),
        &script,
    )
    .unwrap();
    std::fs::write(
        hooks.join("never-whole.txt"),
        "store/
store/index/ids.tsv
docs/gone.md
",
    )
    .unwrap();
    let run = |mode: &str, tool: &str, input: &str| {
        use std::io::Write;
        let payload = format!(
            "{{\"session_id\":\"s1\",\"cwd\":\"/w/app_q1\",\"tool_name\":\"{tool}\",\"tool_input\":{input}}}"
        );
        let mut child = std::process::Command::new("sh")
            .arg(&script)
            .arg(mode)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap_or(-1)
    };
    let bash = |mode: &str, cmd: &str| {
        run(
            mode,
            "Bash",
            &format!("{{\"command\":\"{cmd}\",\"description\":\"d\"}}"),
        )
    };
    // A store-wide grep runs, then the agent is told.
    assert_eq!(bash("pre", "grep -rn loaf store | head"), 0);
    assert_eq!(bash("post", "grep -rn loaf store | head"), 2);
    assert_eq!(bash("post", "sed -i '' s/a/b/ store/loops/BK1.md"), 2);
    assert_eq!(
        run(
            "post",
            "Grep",
            "{\"pattern\":\"loaf\",\"path\":\"store/rules\"}"
        ),
        2
    );
    // Not the store's words: a diff, its own reservations, an exclusion, the plain build.
    assert_eq!(bash("post", "git diff store/loops/BK1.md | grep x"), 0);
    assert_eq!(bash("post", "cat store/reservations/LOOP-Q1.toml"), 0);
    assert_eq!(bash("post", "rg -n x -g !store/** src"), 0);
    assert_eq!(bash("post", "cargo test -q"), 0);
    // A removed document's history; a missing cache under the store is not removed.
    assert_eq!(bash("pre", "git show HEAD~2:docs/gone.md | grep Rule"), 2);
    assert_eq!(bash("pre", "git log -p -- docs/gone.md"), 2);
    assert_eq!(
        bash("pre", "TRAIL_ALLOW=brief git show HEAD~2:docs/gone.md"),
        0
    );
    assert_eq!(bash("pre", "git diff --quiet -- docs/gone.md"), 0);
    assert_eq!(bash("pre", "git show HEAD:store/index/ids.tsv"), 0);
    let log = std::fs::read_to_string(root.path.join(".git/trail/reads.tsv")).unwrap();
    let kinds: Vec<String> = log
        .lines()
        .map(|l| l.split('\t').skip(4).take(2).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        kinds,
        [
            "shell logged",
            "history blocked",
            "history blocked",
            "history brief"
        ],
        "{log}"
    );
    assert!(
        log.lines().all(|l| l.split('\t').nth(1) == Some("app_q1")),
        "{log}"
    );
}

/// a heredoc's body is text, not commands. A commit message or an incident that
/// quotes the history read it records, on a line of its own, was blocked; a command after the
/// heredoc still counts. Global options before the verb are still history, `--pretty` is not
/// `-p`, and the log is capped: past 1 MB it moves to `reads.tsv.1`.
#[test]
fn read_guard_reads_heredocs_as_text_and_caps_its_log() {
    let root = fixture();
    git(&root.path, &["init", "-q"]);
    let hooks = root.path.join(".claude/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("guard.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/claude-code/hooks/guard-read.sh"),
        &script,
    )
    .unwrap();
    std::fs::write(hooks.join("never-whole.txt"), "store/\ndocs/gone.md\n").unwrap();
    // `cmd` is already JSON-escaped: `\\n` is a newline of the command.
    let bash = |cmd: &str| {
        use std::io::Write;
        let payload = format!(
            "{{\"session_id\":\"s1\",\"cwd\":\"/w/app_q1\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":\"{cmd}\",\"description\":\"d\"}}}}"
        );
        let mut child = std::process::Command::new("sh")
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap_or(-1)
    };
    // Text in a heredoc passes, whatever its quoting.
    assert_eq!(
        bash("git commit -F - <<EOF\\nfix\\n\\ngit log -p -- docs/gone.md was the road\\nEOF"),
        0
    );
    assert_eq!(
        bash("trailmark add incident --body - <<'EOF'\\ngit show HEAD~3:docs/gone.md\\nEOF"),
        0
    );
    assert_eq!(
        bash(
            "trailmark add incident --body - <<-\\\"END\\\"\\n\\tgit show HEAD~3:docs/gone.md\\n\\tEND"
        ),
        0
    );
    // A command after the heredoc is a command; a here-string opens no heredoc.
    assert_eq!(
        bash("cat > m.txt <<EOF\\nx\\nEOF\\ngit show HEAD~3:docs/gone.md"),
        2
    );
    assert_eq!(bash("cat <<<x; git show HEAD~3:docs/gone.md"), 2);
    // Options before the verb; `--pretty` is not the patch flag.
    assert_eq!(bash("git --no-pager show HEAD~3:docs/gone.md"), 2);
    assert_eq!(bash("git -c core.pager=cat log -p -- docs/gone.md"), 2);
    assert_eq!(bash("git log --pretty=oneline -- docs/gone.md"), 0);
    // The cap: a full log moves aside, the new line starts a fresh one.
    let dir = root.path.join(".git/trail");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("reads.tsv"), "x".repeat(1_000_001)).unwrap();
    assert_eq!(bash("grep -rn loaf store"), 0);
    let fresh = std::fs::read_to_string(dir.join("reads.tsv")).unwrap();
    assert_eq!(fresh.lines().count(), 1, "{fresh}");
    assert_eq!(
        std::fs::metadata(dir.join("reads.tsv.1")).unwrap().len(),
        1_000_001
    );
}

///  the guard counts only real reads of a record's content. A filter
/// over another command's output (`git status | grep -v store/`), a pattern that names the
/// store, a Glob's listing and the evaluation's cached stores (`~/.cache/trail/eval`, the
/// `stores/<key>/` copies) are not reads; a record given to a reader as a file still is.
#[test]
fn read_guard_counts_only_real_reads_of_records() {
    let root = fixture();
    git(&root.path, &["init", "-q"]);
    let hooks = root.path.join(".claude/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("guard.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/claude-code/hooks/guard-read.sh"),
        &script,
    )
    .unwrap();
    std::fs::write(hooks.join("never-whole.txt"), "store/\n").unwrap();
    // `shown` is the tool's output, which the harness appends to the payload after the tool ran.
    let run_shown = |mode: &str, cwd: &str, tool: &str, input: &str, shown: &str| {
        use std::io::Write;
        let tail = if shown.is_empty() {
            String::new()
        } else {
            format!(",\"tool_response\":{{\"stdout\":\"{shown}\"}},\"tool_use_id\":\"t1\"")
        };
        let payload = format!(
            "{{\"session_id\":\"s1\",\"cwd\":\"{cwd}\",\"tool_name\":\"{tool}\",\"tool_input\":{input}{tail}}}"
        );
        let mut child = std::process::Command::new("sh")
            .arg(&script)
            .arg(mode)
            .env("HOME", "/h")
            .env_remove("TRAIL_EVAL_CACHE")
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait().unwrap().code().unwrap_or(-1)
    };
    let run =
        |mode: &str, cwd: &str, tool: &str, input: &str| run_shown(mode, cwd, tool, input, "");
    // `cmd` is JSON-escaped already.
    let bash = |mode: &str, cmd: &str| {
        run(
            mode,
            "/w/app_q1",
            "Bash",
            &format!("{{\"command\":\"{cmd}\",\"description\":\"d\"}}"),
        )
    };
    let quiet = [
        "git status --short | grep -v store/",
        "git diff --cached --stat | grep -c \\\"store/\\\"",
        "trailmark find loaf | grep store/rules",
        "grep -rlE \\\"store/|trailmark (context|show)\\\" scratchpad/w1",
        "grep -e store/ -n src/main.rs",
        "grep -l '^short:' stores/k8/store/rules/a.md",
        "cat ~/.cache/trail/eval/stores/k8/store/rules/a.md",
        "cd ~/.cache/trail/eval && grep -n short stores-shuffled/k8/store/rules/a.md",
        "sed -n 1,5p /h/.cache/trail/eval/stores/k8/store/loops/BK1.md",
        "awk -F store/ '{print $2}' list.txt",
        // near misses: a listing outside the store, echo, a script that does not read.
        "find src -name '*.md' | xargs cat",
        "find store -exec echo {} +",
        "bash -c 'echo store/rules/2.1.md'",
    ];
    for cmd in quiet {
        assert_eq!(bash("pre", cmd), 0, "{cmd}");
        assert_eq!(bash("post", cmd), 0, "{cmd}");
    }
    let reads = [
        "grep -rn loaf store | head",
        "head -20 store/loops/BR2.md",
        "sed -n 9,18p store/loops/q1.md",
        "grep -n \\\"^areas\\\" store/loops/k9.md",
        "wc -l < store/rules/2.1.md",
        "git log --oneline | grep x; cat store/rules/2.1.md",
        "jq . store/rules/x.json",
        // an awk program that reads a record itself, a `cd` that only moved a subshell,
        // and a `..` that leaves the evaluation's cache are reads of the store.
        "awk 'BEGIN{while((getline l < \\\"store/rules/2.1.md\\\")>0) print l}'",
        "(cd ~/.cache/trail/eval/stores/K && ls); cat store/rules/2.1.md",
        "cat ~/.cache/trail/eval/../../w/app_q1/store/rules/2.1.md",
        "cd ~/.cache/trail/eval/stores/K/../../../../x && head store/rules/2.1.md",
        // a reader fed by a store listing, find -exec, and bash -c.
        "find store/rules -name '*.md' | xargs cat",
        "find store -exec cat {} +",
        "bash -c 'cat store/rules/2.1.md'",
    ];
    for cmd in reads {
        assert_eq!(bash("post", cmd), 2, "{cmd}");
    }
    //... while a subshell that stays in the cache, and an awk program that reads another file,
    // are not.
    for cmd in [
        "(cd ~/.cache/trail/eval/stores/K && grep -n short store/rules/a.md)",
        "(cd ~/.cache/trail/eval/stores/K; head store/loops/BK1.md) | wc -l",
        "awk 'BEGIN{while((getline l < \\\"list.txt\\\")>0) print l}'",
    ] {
        assert_eq!(bash("post", cmd), 0, "{cmd}");
    }
    // A Read whose `..` leaves the cache is the store's own file: still blocked whole.
    assert_eq!(
        run(
            "pre",
            "/w/app_q1",
            "Read",
            "{\"file_path\":\"/h/.cache/trail/eval/../../w/app_q1/store/rules/2.1.md\"}"
        ),
        2
    );
    assert_eq!(bash("pre", "head -20 store/loops/BR2.md"), 0);
    assert_eq!(bash("post", "sed -i '' s/a/b/ store/loops/BK1.md"), 2);
    assert_eq!(
        bash(
            "post",
            "sed -i '' s/a/b/ /h/.cache/trail/eval/stores/K/store/loops/BK1.md"
        ),
        0
    );
    // The tools: a pattern is not a path, a Glob reads nothing, the cache is left alone.
    assert_eq!(
        run(
            "post",
            "/w/app_q1",
            "Grep",
            "{\"pattern\":\"store/\",\"path\":\"src\"}"
        ),
        0
    );
    assert_eq!(
        run(
            "post",
            "/w/app_q1",
            "Grep",
            "{\"pattern\":\"loaf\",\"path\":\"store/rules\"}"
        ),
        2
    );
    assert_eq!(
        run(
            "post",
            "/w/app_q1",
            "Glob",
            "{\"pattern\":\"store/rules/*.md\"}"
        ),
        0
    );
    let cached = "{\"file_path\":\"/h/.cache/trail/eval/stores/k8/store/rules/2.1.md\"}";
    assert_eq!(run("pre", "/w/app_q1", "Read", cached), 0);
    assert_eq!(
        run(
            "pre",
            "/w/app_q1",
            "Read",
            "{\"file_path\":\"/w/app_q1/store/rules/2.1.md\"}"
        ),
        2
    );
    // A relative path read from inside the cache.
    assert_eq!(
        run(
            "post",
            "/h/.cache/trail/eval",
            "Bash",
            "{\"command\":\"head stores/k8/store/rules/2.1.md\"}"
        ),
        0
    );
    // What a command printed is not a command: a line of `trailmark show`'s output that
    // starts like a read of the store told the agent its own command had been logged.
    let printed = "first line\\ngrep -n loaf store/rules/2.1.md\\n  cat store/loops/BK1.md\\n";
    for input in [
        "{\"command\":\"trailmark show GUIDE-2.1\"}",
        "{\"command\":\"ls src\",\"description\":\"d\"}",
    ] {
        assert_eq!(
            run_shown("post", "/w/app_q1", "Bash", input, printed),
            0,
            "{input}"
        );
    }
    // The same text as the command is a read.
    assert_eq!(
        run_shown(
            "post",
            "/w/app_q1",
            "Bash",
            "{\"command\":\"grep -n loaf store/rules/2.1.md\"}",
            "ok"
        ),
        2
    );
    // Only the real reads reached the log (the log is written before the tool runs).
    let log = std::fs::read_to_string(root.path.join(".git/trail/reads.tsv")).unwrap_or_default();
    assert!(
        !log.contains("stores") && !log.contains("grep -v") && !log.contains("trailmark find"),
        "{log}"
    );
    assert_eq!(log.lines().count(), 3, "{log}");
}

///  the adapters are built into the binary. The installed `trailmark` read them
/// from the folder it was built in, so removing that worktree broke `trailmark adapter install`;
/// now a binary run with its source folder gone (here: `CARGO_MANIFEST_DIR` pointing at an
/// empty folder) lists and writes the same bytes. `TRAIL_ADAPTERS` still names a folder.
#[test]
fn adapters_are_built_into_the_binary() {
    let root = prepared();
    let empty = root.path.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let run = |args: &[&str], adapters: Option<&Path>| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_trailmark"));
        cmd.args(args)
            .current_dir(&root.path)
            .env("CARGO_MANIFEST_DIR", &empty)
            .env_remove("TRAIL_ADAPTERS");
        if let Some(dir) = adapters {
            cmd.env("TRAIL_ADAPTERS", dir);
        }
        cmd.output().unwrap()
    };
    let out = run(&["adapter", "install", "claude-code", "--dry-run"], None);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    for f in [
        ".claude/skills/trailmark/SKILL.md",
        ".claude/hooks/trail-session-start.sh",
        ".claude/hooks/trail-guard-read.sh",
        ".claude/hooks/never-whole.txt",
    ] {
        assert!(text.contains(f), "{f}: {text}");
    }
    let out = run(&["adapter", "install", "claude-code", "--force"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/claude-code");
    assert_eq!(
        std::fs::read(root.path.join(".claude/hooks/trail-guard-read.sh")).unwrap(),
        std::fs::read(src.join("hooks/guard-read.sh")).unwrap()
    );
    assert_eq!(
        std::fs::read(root.path.join(".claude/skills/trailmark/SKILL.md")).unwrap(),
        std::fs::read(src.join("SKILL.md")).unwrap()
    );
    // Without a name it says which adapters this build carries; a folder still overrides.
    let out = run(&["adapter", "install"], None);
    assert!(String::from_utf8_lossy(&out.stderr).contains("claude-code, generic"));
    let out = run(
        &["adapter", "install", "generic", "--dry-run"],
        Some(&empty),
    );
    assert_eq!(out.status.code(), Some(2));
}

///  the term index. `trailmark terms` prints each term with its phrases and ids within the
/// budget (cut to fit, saying so), leaves out ids the store does not hold (named on stderr);
/// `context --terms` leads with the records the named terms point to, ranks on the terms'
/// words, says which terms it read and which words named none. A name or a phrase resolves.
#[test]
fn terms_index_prints_within_budget_and_leads_the_bundle() {
    let root = prepared();
    write(
        &root.path,
        "store/terms.toml",
        r#"[[term]]
name = "tray width"
area = "web"
says = ["loaves look cramped", "names squeezed on the board"]
ids = ["GUIDE-2.100", "GUIDE-2.99"]

[[term]]
name = "failing case"
area = "process"
says = ["say what broke"]
ids = ["GUIDE-2.2"]
"#,
    );
    let out = trailmark(&root.path, &["terms"]);
    expect(&out, 0, "terms");
    assert!(
        out.stdout.starts_with("<!-- trailmark:terms tok4="),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "## web\ntray width: loaves look cramped · names squeezed on the board → 2.100\n"
        ),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("9999"));
    assert!(out.stderr.contains("GUIDE-2.99"), "{}", out.stderr);
    let small = trailmark(&root.path, &["terms", "--budget", "10"]);
    assert!(small.stdout.contains("Cut to fit"), "{}", small.stdout);
    let one = trailmark(&root.path, &["terms", "cramped"]);
    assert!(one.stdout.contains("tray width") && !one.stdout.contains("failing case"));

    let task = "the month grid squeezes on narrow screens";
    let plain = trailmark(&root.path, &["context", task, "--explain"]);
    expect(&plain, 0, "context");
    let with = trailmark(
        &root.path,
        &[
            "context",
            "--terms",
            "tray width, say what broke; nonsense",
            task,
            "--explain",
        ],
    );
    expect(&with, 0, "context --terms");
    assert!(
        with.stdout
            .contains("Terms: tray width (1) · failing case (1); not in the index: nonsense"),
        "{}",
        with.stdout
    );
    assert!(
        with.stdout.contains("rule GUIDE-2.100  score"),
        "{}",
        with.stdout
    );
    assert!(with.stdout.contains("terms tray width"), "{}", with.stdout);
    let full = with
        .stdout
        .split("## Rules — full text")
        .nth(1)
        .unwrap_or("");
    assert!(
        full.trim_start().starts_with("GUIDE-2.100") || full.trim_start().starts_with("GUIDE-2.2"),
        "{full}"
    );
    assert!(!plain.stdout.contains("Terms:"));
    // No --terms: the file changes nothing.
    std::fs::remove_file(root.path.join("store/terms.toml")).unwrap();
    let again = trailmark(&root.path, &["context", task, "--explain"]);
    assert_eq!(again.stdout, plain.stdout);
    let none = trailmark(&root.path, &["terms"]);
    assert_eq!(none.code, 1);
}

///  the Now line names running reviews and items beside loops, counts items
/// past three, and points at a command rather than an index file; `find` with a filter and
/// no words lists what the filter holds.
#[test]
fn now_line_names_running_reviews_and_items() {
    let root = prepared();
    expect(
        &trailmark(&root.path, &["add", "review", "--for", "BK2"]),
        0,
        "add review",
    );
    let body = root.path.join("item.md");
    std::fs::write(&body, "A running item.\n").unwrap();
    let body = body.display().to_string();
    for key in ["R1", "R2"] {
        expect(
            &trailmark(
                &root.path,
                &[
                    "add",
                    "item",
                    key,
                    "--title",
                    "Runs",
                    "--field",
                    "asked_by=author",
                    "--field",
                    "status=running",
                    "--body",
                    &body,
                ],
            ),
            0,
            "add item",
        );
    }
    let core = trailmark(&root.path, &["context", "--core"]);
    assert!(
        core.stdout
            .contains("Running: LOOP-BK2, REV-BK2, ITEM-BK2, ITEM-R1, ITEM-R2."),
        "{}",
        core.stdout
    );
    assert!(
        core.stdout
            .contains("(trailmark find --type open --status open)"),
        "{}",
        core.stdout
    );
    expect(
        &trailmark(
            &root.path,
            &[
                "add",
                "item",
                "R3",
                "--title",
                "Runs",
                "--field",
                "asked_by=author",
                "--field",
                "status=running",
                "--body",
                &body,
            ],
        ),
        0,
        "add item",
    );
    let core = trailmark(&root.path, &["context", "--core"]);
    assert!(
        core.stdout.contains(
            "Running: LOOP-BK2, REV-BK2, 4 items marked running (trailmark find --type item --status running)."
        ),
        "{}",
        core.stdout
    );
    let listed = trailmark(
        &root.path,
        &["find", "--type", "item", "--status", "running"],
    );
    expect(&listed, 0, "find by filter");
    assert_eq!(listed.stdout.lines().count(), 4, "{}", listed.stdout);
    expect(&trailmark(&root.path, &["find"]), 2, "find with nothing");
}

fn recall_rule(root: &Path, num: u32, title: &str, rule: &str) {
    write(
        root,
        &format!("store/rules/2.{num}.md"),
        &format!(
            "---\nid: GUIDE-2.{num}\ntype: rule\ntitle: {title}\ndate: 2026-10-27\nstatus: active\nareas: [tests]\nsource: LOOP-BK2\n---\n**Incident (2026-10-27).** It happened.\n\n> **Rule.** {rule}\n"
        ),
    );
}

fn bundle_section<'a>(text: &'a str, head: &str) -> &'a str {
    text.split(head)
        .nth(1)
        .map(|rest| rest.split("\n## ").next().unwrap_or(rest))
        .unwrap_or("")
}

///  a recipe (a fact of kind `recipe`) is printed under "How to run" when one of its
/// `applies_to` tags is a word of the task, never among the traps; an area alone does not
/// bring it, and a bundle carries four at most.
#[test]
fn recipes_print_under_how_to_run_for_the_tasks_they_name() {
    let root = prepared();
    let recipe = |slug: &str, tags: &str, cmd: &str| {
        write(
            &root.path,
            &format!("store/facts/{slug}.md"),
            &format!(
                "---\nid: FACT-{slug}\ntype: fact\ntitle: Recipe {slug}\ndate: 2026-10-27\nstatus: active\nkind: recipe\nscope: repo\napplies_to: [{tags}]\n---\nWhen it applies.\n\n```sh\n{cmd}\n```\n\n**Why:** it was missed.\n\n**How to apply:** copy it.\n"
            ),
        );
    };
    recipe("stack-up", "worktree, web", "tests/stack.sh up");
    recipe("web-only", "web", "bun run --cwd web test");
    for n in 1..=4 {
        recipe(&format!("more-{n}"), "worktree", &format!("echo {n}"));
    }
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
    let out = trailmark(
        &root.path,
        &["context", "the layout of the tray, in my worktree"],
    );
    expect(&out, 0, "context");
    let how = bundle_section(&out.stdout, "## How to run\n");
    assert!(
        how.contains("FACT-stack-up  Recipe stack-up"),
        "{}",
        out.stdout
    );
    assert!(how.contains("tests/stack.sh up"), "{how}");
    assert!(!how.contains("**Why:**"), "{how}");
    assert!(!how.contains("FACT-web-only"), "an area alone: {how}");
    assert_eq!(how.matches("FACT-").count(), 4, "{how}");
    let traps = bundle_section(&out.stdout, "## Gates and traps\n");
    assert!(!traps.contains("Recipe"), "{traps}");
    // No word of the task names a tag: no section.
    let out = trailmark(&root.path, &["context", "the tray on the rack"]);
    assert!(!out.stdout.contains("## How to run"), "{}", out.stdout);
    // A tag is said whole, not stemmed and not scattered.
    // is not `stack`, "before" and "after" in two clauses are not `before-after`.
    recipe("land", "land, stack, before-after", "git merge --ff-only");
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    for task in [
        "the rye loaf on the board",
        "the cards look stacked under the header",
        "before the fade the pill is grey, and after the hover it is white",
    ] {
        let out = trailmark(&root.path, &["context", task]);
        assert!(!out.stdout.contains("FACT-land"), "{task}: {}", out.stdout);
    }
    for task in [
        "land the tray",
        "two stacks at once",
        "the tray's before and after",
    ] {
        let out = trailmark(&root.path, &["context", task]);
        assert!(out.stdout.contains("FACT-land"), "{task}: {}", out.stdout);
    }
    // A loop's own bundle is work in a worktree: its worktree recipes come with it even when
    // the brief is still `Pending.`.
    let out = trailmark(&root.path, &["context", "--loop", "BK1"]);
    let how = bundle_section(&out.stdout, "## How to run\n");
    assert!(how.contains("FACT-stack-up"), "{}", out.stdout);
    assert!(!how.contains("FACT-web-only"), "{how}");
    // A recipe over 400 tok4 is a warning.
    recipe(
        "long",
        "worktree",
        &"echo a long line of commands\n".repeat(80),
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("C14 warning") && out.stdout.contains("a recipe of"),
        "{}",
        out.stdout
    );
}

///  the traps a loop's trailmark misses named come back. `trailmark tag` gives a rule the
/// situations it must surface for; a tag the task names brings a rule that shares no word with
/// it. `trailmark link <loop> <rule> --rel missed` records a miss, and a task like that loop's gets
/// the rule — but not the loop's own bundle, whose misses came after it.
#[test]
fn tags_and_recorded_misses_bring_back_the_traps() {
    let root = prepared();
    recall_rule(
        &root.path,
        106,
        "Ports are chosen before up",
        "Pick five free ports.",
    );
    recall_rule(
        &root.path,
        107,
        "Sentinel alpha omega",
        "Say alpha before omega.",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let task = "the tray on the rack, in my worktree";
    let bundle = |task: &str| trailmark(&root.path, &["context", task]).stdout;
    assert!(!bundle(task).contains("GUIDE-2.106"), "{}", bundle(task));
    let like_bk1 = "the sourdough was weighed lighter than the ticket again";
    // A tag the task names.
    expect(
        &trailmark(
            &root.path,
            &["tag", "GUIDE-2.106", "--add", "worktree,release-stack"],
        ),
        0,
        "tag",
    );
    assert_eq!(
        trailmark(&root.path, &["tag", "GUIDE-2.106", "--add", "Bad Tag"]).code,
        2
    );
    // A loop takes a tag since (earlier work); an open item does not.
    assert_eq!(
        trailmark(&root.path, &["tag", "OPEN-0205", "--add", "x"]).code,
        2
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
    let out = trailmark(&root.path, &["context", task, "--explain"]);
    assert!(out.stdout.contains("GUIDE-2.106"), "{}", out.stdout);
    assert!(out.stdout.contains("tags worktree"), "{}", out.stdout);
    // Both words of a tag must be in the task.
    assert!(!bundle("the tray on a release build").contains("GUIDE-2.106"));
    assert!(bundle("the tray on a release stack").contains("GUIDE-2.106"));

    // A recorded miss.
    assert!(
        !bundle(like_bk1).contains("GUIDE-2.107"),
        "{}",
        bundle(like_bk1)
    );
    expect(
        &trailmark(
            &root.path,
            &["link", "LOOP-BK1", "GUIDE-2.107", "--rel", "missed"],
        ),
        0,
        "missed",
    );
    assert_eq!(
        trailmark(
            &root.path,
            &["link", "GUIDE-2.1", "GUIDE-2.107", "--rel", "missed"]
        )
        .code,
        6,
        "missed is a loop's or a review's"
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
    // the likeness alone is not enough — the task must also name the rule (a tag or
    // its words), or a loop's scope lines ("deploy", "production") carry its traps anywhere.
    let out = trailmark(&root.path, &["context", like_bk1, "--explain"]);
    assert!(!out.stdout.contains("missed by LOOP-BK1"), "{}", out.stdout);
    expect(
        &trailmark(&root.path, &["tag", "GUIDE-2.107", "--add", "ticket"]),
        0,
        "tag the miss (STEP-9)",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["context", like_bk1, "--explain"]);
    assert!(out.stdout.contains("missed by LOOP-BK1"), "{}", out.stdout);
    // A task that names the tag but is not like the loop gets the tag, not the miss.
    let out = trailmark(&root.path, &["context", "the rye search box", "--explain"]);
    assert!(!out.stdout.contains("missed by LOOP-BK1"), "{}", out.stdout);
    // The core's most-cited list counts the `missed` link as a citation; the task's own rule
    // sections are what the bonus moves.
    let rules = |text: &str| {
        format!(
            "{}{}",
            bundle_section(text, "## Rules — full text\n"),
            bundle_section(text, "## Rules — one line each (trailmark show <id>)\n")
        )
    };
    assert!(rules(&bundle(like_bk1)).contains("GUIDE-2.107"));
    assert!(!rules(&bundle("the counter search box on the board")).contains("GUIDE-2.107"));
    let own = trailmark(&root.path, &["context", "--loop", "BK1", "--explain"]);
    assert!(!own.stdout.contains("missed by LOOP-BK1"), "{}", own.stdout);
    // Nor in its review's bundle, which is ranked on the loop's own brief.
    let none = trailmark(&root.path, &["context", "--review", "ZZ9"]);
    assert!(
        none.stderr.contains("trailmark add review --for ZZ9"),
        "{}",
        none.stderr
    );
    let rev = trailmark(&root.path, &["context", "--review", "BK1", "--explain"]);
    assert!(!rev.stdout.contains("missed by LOOP-BK1"), "{}", rev.stdout);

    // A rule linked to the file that broke it; a task naming the file finds it.
    expect(
        &trailmark(
            &root.path,
            &["link", "GUIDE-2.107", "code/note.ts", "--rel", "links"],
        ),
        0,
        "link a path",
    );
    assert_eq!(
        trailmark(
            &root.path,
            &["link", "GUIDE-2.107", "code/gone.ts", "--rel", "links"]
        )
        .code,
        1
    );
    let shown = trailmark(&root.path, &["show", "GUIDE-2.107", "--fields", "links"]);
    assert!(shown.stdout.contains("code/note.ts"), "{}", shown.stdout);
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
}

///  a loop's record lives in its own worktree until it lands, so a bundle read in another
/// checkout names it under "Running in other worktrees"; once this branch has the record, the
/// worktree's stale copy is not listed.
#[test]
fn a_bundle_names_what_runs_in_other_worktrees() {
    let root = prepared();
    git(&root.path, &["init", "-q"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    let path = root.path.with_file_name(format!(
        "{}-other",
        root.path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&path);
    git(
        &root.path,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "other",
            &path.display().to_string(),
        ],
    );
    let other = Root { path };
    let core = trailmark(&root.path, &["context", "--core"]);
    assert!(!core.stdout.contains("other worktrees"), "{}", core.stdout);
    expect(
        &trailmark(
            &other.path,
            &[
                "add",
                "loop",
                "OW1",
                "--title",
                "Elsewhere",
                "--item",
                "none",
                "--platform",
                "web",
            ],
        ),
        0,
        "add loop in the other worktree",
    );
    let name = other
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let core = trailmark(&root.path, &["context", "--core"]);
    assert!(
        core.stdout
            .contains(&format!("Running in other worktrees: LOOP-OW1 ({name}).")),
        "{}",
        core.stdout
    );
    let bundle = trailmark(&root.path, &["context", "the tray on the rack"]);
    assert!(bundle.stdout.contains("LOOP-OW1 ("), "{}", bundle.stdout);
    // The index stays this branch's: the line is not in the committed core.
    let index = std::fs::read_to_string(root.path.join("store/index/core.md")).unwrap();
    assert!(!index.contains("other worktrees"));
    // This branch already knows LOOP-BK2; the other worktree's copy of it is not "elsewhere".
    assert!(!core.stdout.contains("LOOP-BK2 ("), "{}", core.stdout);
}

/// a record added without a part says `Pending.` for good (records are not edited
/// after they are added); `check` names it, outside a running loop or review whose parts
/// `trailmark close` fills. And an area refusal names the areas there are.
#[test]
fn a_pending_part_is_named_and_an_unknown_area_lists_the_areas() {
    let root = prepared();
    expect(
        &trailmark(
            &root.path,
            &[
                "add",
                "rule",
                "--for",
                "BK2",
                "--title",
                "A rule with no text",
                "--areas",
                "tests",
            ],
        ),
        0,
        "add rule without parts",
    );
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout.contains("C12 warning") && out.stdout.contains("still says Pending."),
        "{}",
        out.stdout
    );
    let out = trailmark(
        &root.path,
        &[
            "add",
            "rule",
            "--for",
            "BK2",
            "--title",
            "Wrong area",
            "--areas",
            "nowhere",
        ],
    );
    assert_eq!(out.code, 6, "{}{}", out.stdout, out.stderr);
    let text = format!("{}{}", out.stdout, out.stderr);
    assert!(
        text.contains("the areas are: ") && text.contains("tests"),
        "{text}"
    );
}

fn git_out(root: &Path, args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Dev")
        .env("GIT_AUTHOR_EMAIL", "dev@example.com")
        .env("GIT_COMMITTER_NAME", "Dev")
        .env("GIT_COMMITTER_EMAIL", "dev@example.com")
        .output()
        .expect("git");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

///  two loops in two worktrees each add a loop and a rule, run `trailmark index` and commit
/// their store; the first lands, the orchestrator adds its review in the main checkout without
/// committing, and the second rebases and fast-forwards with no conflict. Before every
/// index header carried the store hash and every block went to one `reservations.toml`, so
/// the second rebase conflicted on all of them and the main checkout's edits refused the
/// fast-forward.
#[test]
fn two_loops_land_one_after_the_other_without_a_conflict() {
    let root = prepared();
    assert!(root.path.join("store/index/.gitignore").is_file());
    git(&root.path, &["init", "-q", "-b", "main"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    let (_, tracked) = git_out(&root.path, &["ls-files", "store/index"]);
    assert!(
        tracked.trim().is_empty(),
        "the index is committed: {tracked}"
    );
    let trees: Vec<Root> = ["a", "b"]
        .iter()
        .map(|k| {
            let path = root.path.with_file_name(format!(
                "{}-{k}",
                root.path.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_dir_all(&path);
            git(
                &root.path,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    k,
                    &path.display().to_string(),
                ],
            );
            Root { path }
        })
        .collect();
    for (tree, key) in trees.iter().zip(["QA1", "QB1"]) {
        let out = trailmark(
            &tree.path,
            &[
                "add",
                "loop",
                key,
                "--title",
                "Scratch",
                "--item",
                "none",
                "--platform",
                "web",
            ],
        );
        expect(&out, 0, "add loop");
        let loop_id = format!("LOOP-{key}");
        let out = trailmark(
            &tree.path,
            &[
                "add",
                "rule",
                "--for",
                &loop_id,
                "--title",
                "A scratch rule",
                "--areas",
                "web",
            ],
        );
        expect(&out, 0, "add rule");
        expect(&trailmark(&tree.path, &["index"]), 0, "index");
        git(&tree.path, &["add", "store"]);
        git(&tree.path, &["commit", "-q", "-m", key]);
    }
    git(&root.path, &["merge", "-q", "--ff-only", "a"]);
    // The orchestrator's review of the first loop, uncommitted in the main checkout.
    expect(
        &trailmark(&root.path, &["add", "review", "--for", "LOOP-QA1"]),
        0,
        "add review",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let (_, dirty) = git_out(
        &root.path,
        &["status", "--porcelain", "--untracked-files=no"],
    );
    assert_eq!(dirty.trim(), "M store/loops/QA1.md", "{dirty}");
    let (rebased, said) = git_out(&trees[1].path, &["rebase", "main"]);
    assert!(rebased, "the second loop's rebase conflicted:\n{said}");
    let (landed, said) = git_out(&root.path, &["merge", "-q", "--ff-only", "b"]);
    assert!(landed, "the second loop did not fast-forward:\n{said}");
    assert!(root.path.join("store/reservations/LOOP-QA1.toml").is_file());
    assert!(root.path.join("store/reservations/LOOP-QB1.toml").is_file());
    assert!(root.path.join("store/reservations/REV-QA1.toml").is_file());
    let out = trailmark(&root.path, &["check"]);
    assert!(
        !out.stdout.contains("C08") && !out.stdout.contains("C11"),
        "{}",
        out.stdout
    );
    for tree in &trees {
        git(
            &root.path,
            &[
                "worktree",
                "remove",
                "--force",
                &tree.path.display().to_string(),
            ],
        );
    }
}

///  `trailmark set` changes an item's or an open item's status, with a note;
/// it refuses the types that have their own command, a status the type does not have, a
/// close without who closed it, and a wait without what it waits on.
#[test]
fn set_changes_an_item_or_an_open_items_status_and_nothing_else() {
    let root = prepared();
    let item = root.path.join("store/items/BK2.md");
    let body_before = std::fs::read_to_string(&item).unwrap();
    let out = trailmark(
        &root.path,
        &[
            "set",
            "ITEM-BK2",
            "status=done",
            "--why",
            "built",
            "--by",
            "LOOP-BK2",
        ],
    );
    expect(&out, 0, "set item done");
    let text = std::fs::read_to_string(&item).unwrap();
    assert!(text.contains("status: done"), "{text}");
    assert!(
        text.contains("status_note: \"2026")
            && text.contains("running → done: built (by LOOP-BK2)"),
        "{text}"
    );
    assert!(
        text.contains("loops: LOOP-BK2") || text.contains("loops: [LOOP-BK2]"),
        "{text}"
    );
    let tail = |t: &str| t.split("\n---\n").nth(1).unwrap_or("").to_string();
    assert_eq!(tail(&text), tail(&body_before), "the body changed");
    // The same status again is a note since (`set_writes_a_note_without_changing_the_status`).
    expect(
        &trailmark(
            &root.path,
            &["set", "ITEM-BK2", "status=done", "--why", "x"],
        ),
        0,
        "already done: a note",
    );
    expect(
        &trailmark(
            &root.path,
            &["set", "ITEM-BK2", "status=waiting", "--why", "x"],
        ),
        2,
        "waiting without waiting_on",
    );
    expect(
        &trailmark(
            &root.path,
            &[
                "set",
                "ITEM-BK2",
                "status=waiting",
                "waiting_on=author",
                "--why",
                "the counter's call",
            ],
        ),
        0,
        "waiting on the author",
    );
    // the second change keeps the first one's why; nothing is overwritten.
    let text = std::fs::read_to_string(&item).unwrap();
    assert!(
        text.contains("done → waiting: the counter's call"),
        "{text}"
    );
    let history = text
        .lines()
        .skip_while(|l| !l.starts_with("status_history:"))
        .take_while(|l| *l != "---")
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        history.contains("running → done: built (by LOOP-BK2)"),
        "{text}"
    );
    assert_eq!(tail(&text), tail(&body_before), "the body changed");
    expect(
        &trailmark(&root.path, &["check"]),
        0,
        "check after two sets",
    );
    expect(
        &trailmark(
            &root.path,
            &["set", "ITEM-BK2", "status=shipped", "--why", "x"],
        ),
        6,
        "a status items do not have",
    );
    expect(
        &trailmark(&root.path, &["set", "ITEM-BK2", "status=done"]),
        2,
        "no --why",
    );
    expect(
        &trailmark(
            &root.path,
            &["set", "LOOP-BK2", "status=done", "--why", "x"],
        ),
        2,
        "a loop is closed",
    );
    expect(
        &trailmark(
            &root.path,
            &["set", "GUIDE-2.100", "status=retired", "--why", "x"],
        ),
        2,
        "a rule retires",
    );
    expect(
        &trailmark(
            &root.path,
            &["set", "OPEN-0214", "status=closed", "--why", "x"],
        ),
        2,
        "a close names who closed it",
    );
    let out = trailmark(
        &root.path,
        &[
            "set",
            "OPEN-0214",
            "status=wontfix",
            "--why",
            "the spare tray is gone",
        ],
    );
    expect(&out, 0, "wontfix");
    let open = std::fs::read_to_string(root.path.join("store/open/0214.md")).unwrap();
    assert!(
        open.contains("## Why not\nthe spare tray is gone\n"),
        "{open}"
    );
    let out = trailmark(
        &root.path,
        &[
            "set",
            "OPEN-0205",
            "status=closed",
            "--why",
            "done",
            "--by",
            "author",
        ],
    );
    expect(&out, 0, "closed by the author");
    let open = std::fs::read_to_string(root.path.join("store/open/0205.md")).unwrap();
    assert!(open.contains("closed_by: author"), "{open}");
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["check"]);
    assert!(!out.stdout.contains(" error:"), "{}", out.stdout);
    let help = trailmark::run(&["help".to_string(), "set".to_string()], "");
    assert!(
        help.stdout.contains("status=") && help.stdout.contains("Example"),
        "{}",
        help.stdout
    );
}

///  an index file git still tracks while the indexes are a local cache is named, with
/// the command that untracks it.
#[test]
fn a_tracked_index_is_a_warning_when_the_index_is_a_cache() {
    let root = prepared();
    git(&root.path, &["init", "-q"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["add", "-f", "store/index/core.md"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    let out = trailmark(&root.path, &["check"]);
    assert!(out.stdout.contains("C22"), "{}", out.stdout);
    assert!(out.stdout.contains("git rm -r --cached"), "{}", out.stdout);
}

///  a fact comes into a bundle when the task's words say one of its tags, as a rule or a
/// recipe does; before, only an area or a platform of the task brought a tagged fact, so the
/// A fact tagged for the oven reaches a task that names the oven.
#[test]
fn a_fact_comes_by_a_tag_the_task_says_and_not_otherwise() {
    let root = prepared();
    write(
        &root.path,
        "store/facts/yeast-timer.md",
        "---\nid: FACT-yeast-timer\ntype: fact\ntitle: The proofing timer is set by the baker\ndate: 2026-02-11\nstatus: active\nkind: trap\nscope: repo\napplies_to: [timer]\n---\nSet it when the dough goes in the tub.\n\n**Why:** a guessed minute overproofs the loaf.\n\n**How to apply:** start the timer from the card, not from memory.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["context", "start the proofing timer"]);
    expect(&out, 0, "context");
    assert!(out.stdout.contains("FACT-yeast-timer"), "{}", out.stdout);
    let out = trailmark(&root.path, &["context", "the morning loaf"]);
    expect(&out, 0, "context");
    assert!(!out.stdout.contains("FACT-yeast-timer"), "{}", out.stdout);
    // Said, not stemmed: "timers" is the tag's plural; "timed" is not the tag. A fact is
    // also ranked by its words with the rules, so a stem may list it there; the tag's
    // place, the traps, stays shut.
    let out = trailmark(&root.path, &["context", "the label checked on the rack"]);
    assert!(
        !bundle_section(&out.stdout, "## Gates and traps\n").contains("FACT-yeast-timer"),
        "{}",
        out.stdout
    );
}

/// a local index is rebuilt by the first read that finds it missing or stale. After
/// the indexes left git, so a clone had none and a pull or a landed loop left the old one
/// in place; `context` trusted a stale `backlinks.tsv` until someone knew to run `trailmark index`.
/// A checkout whose git still tracks the index (a branch cut before ) is never rewritten
/// by a read, or its pull would refuse the modified files.
#[test]
fn a_missing_or_stale_local_index_is_rebuilt_by_the_first_read() {
    let root = fixture();
    let index = root.path.join("store/index");
    assert!(!index.exists());
    expect(&trailmark(&root.path, &["context", "--core"]), 0, "core");
    for name in [
        ".gitignore",
        ".key",
        "core.md",
        "backlinks.tsv",
        "rules-hot.md",
    ] {
        assert!(index.join(name).is_file(), "{name} was not built");
    }
    let key = std::fs::read_to_string(index.join(".key")).unwrap();
    let links = std::fs::read_to_string(index.join("backlinks.tsv")).unwrap();
    assert!(!links.contains("GUIDE-2.2\tcode/"), "{links}");

    // Fresh: a second read writes nothing.
    std::fs::write(index.join("core.md"), "kept\n").unwrap();
    expect(
        &trailmark(&root.path, &["context", "a layout task"]),
        0,
        "fresh",
    );
    assert_eq!(
        std::fs::read_to_string(index.join("core.md")).unwrap(),
        "kept\n"
    );

    // A code citation and a new rule, as a pull brings them: the next read rebuilds.
    write(&root.path, "code/more.ts", "// See GUIDE-2.2.\n");
    expect(
        &trailmark(&root.path, &["context", "a layout task"]),
        0,
        "stale",
    );
    let links = std::fs::read_to_string(index.join("backlinks.tsv")).unwrap();
    assert!(links.contains("GUIDE-2.2\tcode/more.ts"), "{links}");
    assert_ne!(std::fs::read_to_string(index.join(".key")).unwrap(), key);
    assert_ne!(
        std::fs::read_to_string(index.join("core.md")).unwrap(),
        "kept\n"
    );

    // In git the code counts by commit (a walk of a large tree was the cost): a citation
    // counts once it is committed, as a pull brings it.
    git(&root.path, &["init", "-q"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    write(&root.path, "code/third.ts", "// See GUIDE-2.3.\n");
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "a citation"]);
    expect(
        &trailmark(&root.path, &["context", "a layout task"]),
        0,
        "pulled",
    );
    let links = std::fs::read_to_string(index.join("backlinks.tsv")).unwrap();
    assert!(links.contains("GUIDE-2.3\tcode/third.ts"), "{links}");

    // A checkout that still tracks the index is read, never rewritten.
    git(&root.path, &["add", "-f", "store/index"]);
    git(&root.path, &["commit", "-q", "-m", "the index, tracked"]);
    write(&root.path, "code/fourth.ts", "// See GUIDE-2.4.\n");
    git(&root.path, &["add", "code/fourth.ts"]);
    git(&root.path, &["commit", "-q", "-m", "another citation"]);
    expect(
        &trailmark(&root.path, &["context", "a layout task"]),
        0,
        "tracked",
    );
    let links = std::fs::read_to_string(index.join("backlinks.tsv")).unwrap();
    assert!(!links.contains("code/fourth.ts"), "{links}");
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&root.path)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        status.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
}

///  a loop added without a brief says `Pending.` until its close, and its bundle was
/// ranked on the title alone. It is ranked on its item too, and the target says how to
/// ask on the task's words; a brief given at `add` is the bundle's brief at once.
#[test]
fn a_pending_brief_ranks_on_the_item_and_says_how_to_ask() {
    let root = prepared();
    recall_rule(
        &root.path,
        6,
        "Month names decide the tray width",
        "Measure the longest month name.",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let add = |key: &str, item: &str, extra: &[&str]| {
        let mut args = vec![
            "add",
            "loop",
            key,
            "--title",
            "Next slice",
            "--item",
            item,
            "--platform",
            "web",
        ];
        args.extend_from_slice(extra);
        expect(&trailmark(&root.path, &args), 0, "add loop");
    };
    add("Q1", "ITEM-BK2", &[]);
    let out = trailmark(&root.path, &["context", "--loop", "Q1"]);
    expect(&out, 0, "context");
    assert!(
        out.stdout.contains("ranked on the title and its item"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("trailmark context --loop Q1 \""),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("GUIDE-2.6"),
        "the item's words rank: {}",
        out.stdout
    );
    // A task given with --loop is the words the hint asks for: no hint then.
    let out = trailmark(&root.path, &["context", "--loop", "Q1", "the month names"]);
    assert!(
        !out.stdout.contains("brief is not written yet"),
        "{}",
        out.stdout
    );
    // No item: the title alone, and the hint says so.
    add("Q2", "none", &[]);
    let out = trailmark(&root.path, &["context", "--loop", "Q2"]);
    assert!(
        out.stdout.contains("ranked on the title alone"),
        "{}",
        out.stdout
    );
    // A brief at add.
    write(
        &root.path,
        "brief.md",
        "Fit the longest loaf name on the rack.\n",
    );
    let brief = root.path.join("brief.md").display().to_string();
    add("Q3", "none", &["--part", &format!("brief={brief}")]);
    let out = trailmark(&root.path, &["context", "--loop", "Q3"]);
    assert!(
        out.stdout.contains("Fit the longest loaf name"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("brief is not written yet"),
        "{}",
        out.stdout
    );
}

/// A taken key names its holder and the next key no record and no block holds.
#[test]
fn a_taken_loop_key_names_the_next_free_one() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &[
            "add",
            "loop",
            "BK1",
            "--title",
            "Again",
            "--item",
            "none",
            "--platform",
            "web",
        ],
    );
    expect(&out, 5, "taken");
    assert!(out.stderr.contains("Morning bake, done"), "{}", out.stderr);
    // BK2 is a record, so the next free one is BK3.
    assert!(
        out.stderr.contains("the next free key is BK3"),
        "{}",
        out.stderr
    );
    // a review takes its loop's key; a second one is not sent to another loop's key.
    expect(
        &trailmark(&root.path, &["add", "review", "--for", "BK2"]),
        0,
        "review",
    );
    let out = trailmark(&root.path, &["add", "review", "--for", "BK2"]);
    expect(&out, 5, "review taken");
    assert!(!out.stderr.contains("next free key"), "{}", out.stderr);
    assert!(
        out.stderr.contains("Review of BK2, running"),
        "{}",
        out.stderr
    );
}

///  `set` with the status a record already has, or with none, writes a note and nothing
/// else.
#[test]
fn set_writes_a_note_without_changing_the_status() {
    let root = prepared();
    let open = root.path.join("store/open/0204.md");
    let before = std::fs::read_to_string(&open).unwrap();
    let out = trailmark(
        &root.path,
        &[
            "set",
            "OPEN-0204",
            "status=closed",
            "--why",
            "correction: measured twice",
        ],
    );
    expect(&out, 0, "note on a closed item");
    let after = std::fs::read_to_string(&open).unwrap();
    assert!(
        after.contains("note: correction: measured twice"),
        "{after}"
    );
    for key in ["closed_by", "closed:"] {
        let line = |t: &str| t.lines().find(|l| l.starts_with(key)).map(str::to_string);
        assert_eq!(line(&before), line(&after), "{key} kept");
    }
    let out = trailmark(
        &root.path,
        &[
            "set",
            "ITEM-BK2",
            "--why",
            "still the tray",
            "--by",
            "LOOP-BK2",
        ],
    );
    expect(&out, 0, "a note alone");
    let item = std::fs::read_to_string(root.path.join("store/items/BK2.md")).unwrap();
    assert!(item.contains("status: running"), "{item}");
    assert!(
        !item.contains("loops:"),
        "a note does not add the loop: {item}"
    );
    assert_eq!(
        trailmark(&root.path, &["set", "ITEM-BK2"]).code,
        2,
        "nothing to write"
    );
    expect(&trailmark(&root.path, &["check"]), 0, "check");
}

/// An earlier loop is a miss too. `link --rel missed` accepts a loop. A loop a past alike
/// loop missed, or tagged for the task's words, is listed under the target as "Earlier work".
#[test]
fn an_earlier_loop_is_a_miss_and_comes_back_as_earlier_work() {
    let root = prepared();
    write(
        &root.path,
        "store/loops/CD1.md",
        "---\nid: LOOP-CD1\ntype: loop\ntitle: CD1 — the counter cards share one frame\ndate: 2026-04-03\nstatus: done\nlegacy: true\nitem: none\n---\n## CD1 — the counter cards share one frame\n\nThe cards were lined up once.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let task = "the morning bake on the counter cards";
    assert!(!trailmark(&root.path, &["context", task])
        .stdout
        .contains("Earlier work"));
    expect(
        &trailmark(
            &root.path,
            &["link", "LOOP-BK1", "LOOP-CD1", "--rel", "missed"],
        ),
        0,
        "a loop is a miss",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["context", task]);
    assert!(
        out.stdout.contains("Earlier work: LOOP-CD1") && out.stdout.contains("missed by LOOP-BK1"),
        "{}",
        out.stdout
    );
    // A migrated loop has no parts: the pointer shows it whole.
    assert!(
        out.stdout.contains("`trailmark show LOOP-CD1`"),
        "{}",
        out.stdout
    );
    // Likeness alone is not enough: a task that does not name the loop does not get it.
    let out = trailmark(
        &root.path,
        &["context", "the label overflowed once rewritten in rye"],
    );
    assert!(!out.stdout.contains("Earlier work"), "{}", out.stdout);
    // A tag the task says brings it without a miss.
    expect(
        &trailmark(&root.path, &["tag", "LOOP-CD1", "--add", "frame"]),
        0,
        "tag a loop",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(&root.path, &["context", "a frame around the search box"]);
    assert!(out.stdout.contains("tagged for this"), "{}", out.stdout);
    expect(&trailmark(&root.path, &["check"]), 0, "check");
}

/// A decision's number is the next after every worktree's, so two loops
/// next door that each record a ruling take two numbers.
#[test]
fn decisions_next_door_take_distinct_numbers() {
    let root = prepared();
    git(&root.path, &["init", "-q"]);
    git(&root.path, &["add", "-A"]);
    git(&root.path, &["commit", "-q", "-m", "init"]);
    let trees: Vec<Root> = (1..=2)
        .map(|k| {
            let path = root.path.with_file_name(format!(
                "{}-dec{k}",
                root.path.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_dir_all(&path);
            git(
                &root.path,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("dec{k}"),
                    &path.display().to_string(),
                ],
            );
            Root { path }
        })
        .collect();
    write(&trees[0].path, "d.md", "Dates are the reader's locale.\n");
    write(&trees[1].path, "d.md", "Dates are the reader's locale.\n");
    let mut ids = Vec::new();
    for tree in &trees {
        let d = tree.path.join("d.md").display().to_string();
        let out = trailmark(
            &tree.path,
            &[
                "add",
                "decision",
                "--title",
                "A ruling",
                "--field",
                "decided_by=author",
                "--field",
                "basis=silence",
                "--part",
                &format!("decision={d}"),
                "--part",
                &format!("why={d}"),
            ],
        );
        expect(&out, 0, "add decision");
        ids.push(out.stdout.lines().next().unwrap_or_default().to_string());
    }
    assert_eq!(ids[0], "DEC-0032", "after the fixture's DEC-0031: {ids:?}");
    assert_eq!(ids[1], "DEC-0033", "after the worktree next door: {ids:?}");
}

/// A signed waiver names its path and the cited text; its line is a hint. Code edited
/// above a waived citation moved it, and `trailmark check` broke until the line was updated.
/// The waiver still covers one citation, the one nearest its line:
/// a second citation of the same text is not waived by it.
#[test]
fn a_waiver_follows_its_citation_when_lines_move() {
    let root = prepared();
    let code = |above: usize, twice: bool| {
        let mut text = "// filler\n".repeat(above);
        text.push_str("export const a = 1; // see §2.999\n");
        if twice {
            text.push_str(&"// more\n".repeat(40));
            text.push_str("export const b = 2; // see §2.999\n");
        }
        write(&root.path, "code/waived.ts", &text);
    };
    write(
        &root.path,
        "store/_migration/waivers.toml",
        "[[waiver]]\ncode = \"C10\"\npath = \"code/waived.ts\"\nline = 3\nwritten = \"§2.999\"\nby = \"the author, signed\"\ndate = \"2026-10-26\"\nwhy = \"no record to point at\"\n",
    );
    let c10 = || {
        trailmark(&root.path, &["check"])
            .stdout
            .lines()
            .filter(|l| l.contains("C10 error"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    code(2, false);
    assert!(c10().is_empty(), "{:?}", c10());
    // Twenty-five lines added above: the same citation, still waived.
    code(27, false);
    assert!(c10().is_empty(), "{:?}", c10());
    // A second citation of the same text further down is a finding of its own.
    code(27, true);
    let left = c10();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].contains("code/waived.ts:69"), "{left:?}");
    // a new citation added nearer the waiver's line than the moved one takes the
    // waiver, and the moved one is the finding: one waiver covers one citation, so a new
    // citation of the same text always leaves a finding, never silence.
    let mut text = "// filler\n".repeat(8);
    text.push_str("export const c = 3; // see §2.999\n");
    text.push_str(&"// filler\n".repeat(18));
    text.push_str("export const a = 1; // see §2.999\n");
    write(&root.path, "code/waived.ts", &text);
    let left = c10();
    assert_eq!(left.len(), 1, "{left:?}");
    // The signature fields are read as they are; the file is not rewritten.
    let w = std::fs::read_to_string(root.path.join("store/_migration/waivers.toml")).unwrap();
    assert!(
        w.contains("line = 3\n") && w.contains("by = \"the author, signed\""),
        "{w}"
    );
}

///  the records around the task's convention come one hop from the best match, printed as
/// ids with a title under "Related". A link field brings its record on one seed; a citation
/// alone needs two of the task's words in the neighbour's title, or a citation of a loaf
/// rule would bring it into any bundle that matched the citing rule.
#[test]
fn related_records_come_one_hop_from_the_best_match() {
    let root = prepared();
    recall_rule(
        &root.path,
        101,
        "The ledger lock is taken before a reservation",
        "Take the ledger lock first (GUIDE-2.102, GUIDE-2.103).",
    );
    recall_rule(&root.path, 102, "Sentinel zeta", "Say zeta.");
    recall_rule(
        &root.path,
        103,
        "A reservation ledger read twice",
        "Read it once.",
    );
    write(
        &root.path,
        "store/decisions/0040.md",
        "---\nid: DEC-0040\ntype: decision\ntitle: Sentinel kappa\ndate: 2026-10-30\nstatus: active\ndecided_by: author\nbasis: silence\nlinks: [GUIDE-2.101]\n---\n## Decision\n\nSay kappa.\n\n## Why\n\nA test.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    expect(&trailmark(&root.path, &["check"]), 0, "check");
    let out = trailmark(
        &root.path,
        &["context", "take the ledger lock before reserving numbers"],
    );
    expect(&out, 0, "context");
    let related = bundle_section(
        &out.stdout,
        "## Related — one hop from the best matches (trailmark show <id>)\n",
    );
    // The decision links the rule: one seed is enough. It shares no word with the task.
    assert!(
        related.contains("DEC-0040  Sentinel kappa (← GUIDE-2.101)"),
        "{}",
        out.stdout
    );
    // A citation, and its title holds two of the task's words (ledger, reservation).
    assert!(out.stdout.contains("GUIDE-2.103"), "{}", out.stdout);
    // A citation of a rule that holds none of them is not listed (its id is in the citing
    // rule's text, which is printed in full).
    assert!(!related.contains("GUIDE-2.102"), "{}", out.stdout);
    assert!(!out.stdout.contains("## Decision\n"), "{}", out.stdout);
    // A task that names a file finds the rule linked to it, among the related.
    expect(
        &trailmark(
            &root.path,
            &["link", "GUIDE-2.102", "code/note.ts", "--rel", "links"],
        ),
        0,
        "link a file",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(
        &root.path,
        &[
            "context",
            "take the ledger lock before code/note.ts reserves",
        ],
    );
    assert!(out.stdout.contains("GUIDE-2.102"), "{}", out.stdout);
}

///  a task whose words the store does not hold, or that no record holds two of, is told
/// so with the nearest conventions instead of a padded list; a word one letter off is read as
/// the store's word.
#[test]
fn a_weak_match_says_did_you_mean_and_a_typo_is_read_as_the_word() {
    let root = prepared();
    let out = trailmark(&root.path, &["context", "quokka marimba"]);
    expect(&out, 0, "context");
    assert!(out.stdout.contains("## Did you mean"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("No convention holds any of this task's words"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("## Rules — full text"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("## Rules — one line each"),
        "{}",
        out.stdout
    );
    // A task that matches is not told so.
    let out = trailmark(&root.path, &["context", "the tray fits on the rack"]);
    assert!(!out.stdout.contains("## Did you mean"), "{}", out.stdout);
    assert!(out.stdout.contains("GUIDE-2.100"), "{}", out.stdout);
    // "trayy" is one edit from "tray".
    let out = trailmark(
        &root.path,
        &["context", "the trayy fits on the rack", "--explain"],
    );
    assert!(!out.stdout.contains("## Did you mean"), "{}", out.stdout);
    assert!(out.stdout.contains("rule GUIDE-2.100 "), "{}", out.stdout);
}

/// the spelling fallback reads a slip (a letter dropped or added, two neighbours
/// swapped) as the store's word, and never a substitution: "faint" is not "paint". Over the
/// repo's own vocabulary the first version rewrote one word in eight into another.
#[test]
fn a_slip_is_read_as_the_word_and_a_substitution_is_not() {
    let root = prepared();
    // A word is a target when three records use it: two more facts say "sourdough".
    for n in 1..=2 {
        write(
            &root.path,
            &format!("store/facts/cal{n}.md"),
            &format!("---\nid: FACT-cal{n}\ntype: fact\ntitle: A sourdough note {n}\ndate: 2026-10-27\nstatus: active\nkind: trap\nscope: repo\n---\nText.\n\n**How to apply:** the sourdough again.\n"),
        );
    }
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    // The task is the one word, so nothing else can match the rule.
    for slip in ["sourdugh", "suordough", "sourdoughh"] {
        let out = trailmark(&root.path, &["context", slip, "--explain"]);
        expect(&out, 0, "context");
        assert!(
            out.stdout.contains("rule GUIDE-2.100 ") && !out.stdout.contains("## Did you mean"),
            "{slip}: {}",
            out.stdout
        );
    }
    for other in ["calender", "xalendar"] {
        let out = trailmark(&root.path, &["context", other, "--explain"]);
        expect(&out, 0, "context");
        assert!(
            out.stdout.contains("## Did you mean") && !out.stdout.contains("rule GUIDE-2.100 "),
            "{other}: {}",
            out.stdout
        );
    }
}

/// a migrated memory's title is its slug, which begins with the project's name; a task
/// that names the project must not list every memory by that word.
#[test]
fn the_projects_name_is_not_a_word_of_a_memorys_slug() {
    let root = prepared();
    write(
        &root.path,
        "store/facts/fixture-rye-before-salt.md",
        "---\nid: FACT-rye-before-salt\ntype: fact\ntitle: fixture-rye-before-salt\ndate: 2026-02-18\nstatus: active\nkind: trap\nscope: repo\n---\nSalt waits until the dough has had its first rise.\n\n**How to apply:** rye before the salt.\n",
    );
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(
        &root.path,
        &["context", "the fixture tray layout on the rack"],
    );
    expect(&out, 0, "context");
    assert!(
        !out.stdout.contains("FACT-rye-before-salt"),
        "{}",
        out.stdout
    );
    let out = trailmark(&root.path, &["context", "rye before the salt"]);
    expect(&out, 0, "context");
    assert!(
        out.stdout.contains("FACT-rye-before-salt"),
        "{}",
        out.stdout
    );
}

/// A decision is ranked with the rules. It had no way into a bundle but its loop's link.
/// The words a task's tags lead to count a little.
#[test]
fn decisions_rank_with_rules_and_tags_lead_to_their_titles_words() {
    let root = prepared();
    let out = trailmark(
        &root.path,
        &["context", "rye stays on the standing menu", "--explain"],
    );
    assert!(out.stdout.contains("DEC-0031"), "{}", out.stdout);
    // Two rules tagged `worktree` whose titles say "compose ports"; a third that says it and
    // holds none of the task's words gains from the tag's words, and says so; a task without
    // the tag gives it nothing.
    recall_rule(&root.path, 104, "Compose ports before up", "Pick them.");
    recall_rule(
        &root.path,
        105,
        "Compose ports of a second stack",
        "Pick others.",
    );
    recall_rule(
        &root.path,
        108,
        "Compose ports collide between two checkouts",
        "Count them.",
    );
    for id in ["GUIDE-2.104", "GUIDE-2.105"] {
        expect(
            &trailmark(&root.path, &["tag", id, "--add", "worktree"]),
            0,
            "tag",
        );
    }
    expect(&trailmark(&root.path, &["index"]), 0, "index");
    let out = trailmark(
        &root.path,
        &["context", "a tray in my worktree", "--explain"],
    );
    let line = out
        .stdout
        .lines()
        .find(|l| l.contains(" GUIDE-2.108 ") && l.contains("score"))
        .unwrap_or_default()
        .to_string();
    assert!(line.contains("by tags' words"), "{}", out.stdout);
    let out = trailmark(&root.path, &["context", "a tray", "--explain"]);
    assert!(!out.stdout.contains("by tags' words"), "{}", out.stdout);
}

/// C23: `trailmark init` sets `[code] include = ["src/**"]`. A dead id in `lib/` was unreported,
/// and `0 code references` was the only sign the scan had found nothing.
#[test]
fn a_code_scan_that_found_nothing_says_so() {
    static N: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "trail-scan-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    let root = Root { path };
    let toml = root.path.join("trail.toml");
    expect(&trailmark(&root.path, &["init"]), 0, "init");
    expect(
        &trailmark(
            &root.path,
            &[
                "add",
                "loop",
                "P1",
                "--title",
                "First slice",
                "--item",
                "none",
                "--platform",
                "docs",
            ],
        ),
        0,
        "add loop",
    );

    // The citation is outside `include`, so no file matched at all.
    write(&root.path, "lib/a.ts", "// RULE-999\n");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "a warning does not fail");
    assert!(
        out.stdout
            .contains("trail.toml: C23 warning: no file matched [code] include (src/**)"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("set [code] include"), "{}", out.stdout);
    assert!(out.stdout.contains("0 code references"), "{}", out.stdout);

    // Files matched and none of them cites an id: the dead id is still not reported.
    write(&root.path, "src/a.ts", "const a = 1;\n");
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout
            .contains("C23 warning: 1 file matched [code] include (src/**) and it cites no id"),
        "{}",
        out.stdout
    );
    write(&root.path, "src/b.ts", "const b = 2;\n");
    let out = trailmark(&root.path, &["check"]);
    assert!(
        out.stdout
            .contains("C23 warning: 2 files matched [code] include (src/**) and none cites an id"),
        "{}",
        out.stdout
    );

    // `--format json` carries it, and `--strict` fails on it like any warning.
    let out = trailmark(&root.path, &["check", "--format", "json"]);
    assert!(out.stdout.contains("\"C23\""), "{}", out.stdout);
    expect(&trailmark(&root.path, &["check", "--strict"]), 1, "strict");

    // Point `include` at the code: the dead id is reported, and the warning has gone.
    replace(&toml, r#"include = ["src/**"]"#, r#"include = ["lib/**"]"#);
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 1, "dead id");
    assert!(
        out.stdout.contains("C10") && out.stdout.contains("RULE-999"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("C23"), "{}", out.stdout);

    // `include = []` is a statement: no code to scan, nothing to warn about.
    replace(&toml, r#"include = ["lib/**"]"#, "include = []");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "include = []");
    assert!(!out.stdout.contains("C23"), "{}", out.stdout);

    // No `include` key says nothing, so the scan's emptiness is still worth saying.
    replace(&toml, "include = []\n", "");
    let out = trailmark(&root.path, &["check"]);
    expect(&out, 0, "no include");
    assert!(
        out.stdout
            .contains("C23 warning: trail.toml has no [code] include"),
        "{}",
        out.stdout
    );
}
