//! `trailmark add <type> --help`: what each type REQUIRES, in the terms of the command line.
//!
//! A cold reader met the same wall: `help add` listed flag names, and
//! `add open` was refused four times in turn — `--kind`, `--areas`, `## What`, `## How to
//! close` — each refusal naming one missing thing. This table says all of them at once, with
//! an example that runs. The checker (`check.rs`, C02 / C07 / C12) stays the authority; the
//! acceptance test `add_help_examples_run_and_name_every_required_field` runs every example
//! below in a fresh store and drops each required field in turn, so the two cannot drift.

pub struct TypeHelp {
    pub name: &'static str,
    /// What the record is, and the id it gets.
    pub what: &'static str,
    /// `(flag or part, meaning)`. A missing field is refused; a missing part is written as
    /// `Pending.`, which `check` warns about outside a running loop or review,
    /// whose parts `trailmark close` fills.
    pub required: &'static [(&'static str, &'static str)],
    pub optional: &'static [(&'static str, &'static str)],
    /// One command that succeeds in a store that has `LOOP-P1` (and its block). Part files are
    /// paths, written beforehand.
    pub example: &'static str,
    pub note: &'static str,
}

pub const TYPES: &[TypeHelp] = &[
    TypeHelp {
        name: "rule",
        what: "a lesson a loop learned. Id RULE-N by default (the next number in the --for loop's block); a project with [ids] rule_style = \"section\" mints <prefix>-2.N.",
        required: &[
            ("--for <LOOP-KEY>", "the loop or review it came from (source)"),
            ("--title \"<the rule as a claim>\"", "one line"),
            ("--areas <a,b>", "names from trail.toml [areas]"),
            ("--part incident=<file>", "what happened (the template adds **Incident (date).**)"),
            ("--part rule=<file>", "what to do from now on (the template adds > **Rule.**)"),
        ],
        optional: &[
            ("--field short=\"<≤72 chars>\"", "the index line"),
            ("--field applies_to=<tags>", "situations it must surface for: areas, platforms, words such as worktree, stack, touch"),
            ("--field links=<ids or paths>", "see also; a path must exist (C05)"),
            ("--figure \"<path in the store> | <caption>\"", "a picture or a short film"),
        ],
        example: "trailmark add rule --for P1 --title \"Labels fit the longest locale\" --areas web --part incident=inc.md --part rule=rule.md",
        note: "",
    },
    TypeHelp {
        name: "open",
        what: "something owed: a risk, a follow-up, a step, an unverified claim, a bug. Id OPEN-NNNN from the --for loop's block.",
        required: &[
            ("--for <LOOP-KEY|REV-KEY>", "who opened it (opened_by)"),
            ("--title \"<one line>\"", ""),
            ("--areas <a,b>", "names from trail.toml [areas]"),
            ("--kind <kind>", "risk | follow-up | owed-step | unverified | bug"),
            ("--severity <s>", "high | medium | low"),
            ("--owner <who>", "author | orchestrator | next-loop | agent (the index sorts the first three first)"),
            ("--part what=<file>", "## What: what is owed and why"),
            ("--part how-to-close=<file>", "## How to close: the act or the test that closes it"),
        ],
        optional: &[],
        example: "trailmark add open --for P1 --title \"The label is unverified on the bag\" --areas web --kind unverified --severity low --owner next-loop --part what=what.md --part how-to-close=how.md",
        note: "Close it later with `trailmark link <LOOP-ID> <OPEN-ID> --rel closes`.",
    },
    TypeHelp {
        name: "decision",
        what: "a ruling. Id DEC-NNNN, the next free number.",
        required: &[
            ("--title \"<the ruling>\"", ""),
            ("--field decided_by=<who>", "author, or who ruled"),
            ("--field basis=<b>", "word | silence | ruling; word also needs --field quote=\"<what was said, ≤300 chars>\""),
            ("--part decision=<file>", "## Decision"),
            ("--part why=<file>", "## Why"),
        ],
        optional: &[("--field links=<ids>", "")],
        example: "trailmark add decision --title \"Rye stays on the menu\" --field decided_by=author --field basis=silence --part decision=decision.md --part why=why.md",
        note: "",
    },
    TypeHelp {
        name: "discussion",
        what: "a remark or a brief as it was said. Id DSC-NNNN.",
        required: &[
            ("--title \"<one line>\"", ""),
            ("--field from=<who>", "author, or who said it"),
            ("--body <file>", "the remarks (`--body -` reads stdin)"),
        ],
        optional: &[("--areas <a,b>", "")],
        example: "trailmark add discussion --title \"Remarks on the label\" --field from=author --areas web --body notes.md",
        note: "",
    },
    TypeHelp {
        name: "item",
        what: "a piece of work in the queue. Id ITEM-<KEY> from the positional key.",
        required: &[
            ("<KEY>", "positional: 1-4 letters and digits (`P1`)"),
            ("--title \"<one line>\"", ""),
            ("--field asked_by=<who>", "author, or who asked"),
            ("--field order=<n>", "its place in the queue (a new item is queued)"),
            ("--body <file>", "a paragraph: what and why"),
        ],
        optional: &[
            ("--field status=<s>", "queued | running | built | waiting (needs waiting_on) | parked | done | dropped"),
            ("--field decisions=<ids>", ""),
        ],
        example: "trailmark add item Q1 --title \"The label fits the bag\" --field asked_by=author --field order=1 --body item.md",
        note: "",
    },
    TypeHelp {
        name: "loop",
        what: "a piece of work being done; its record is the loop report. Id LOOP-<KEY>; reserves 10 rule and 10 open numbers.",
        required: &[
            ("<KEY>", "positional: 1-4 letters, digits, an optional lowercase letter (`B12`)"),
            ("--title \"<one line>\"", ""),
            ("--item <ITEM-ID|none>", "the item it works on"),
            ("--platform <p,q>", "web | server | mobile | infra | docs"),
        ],
        optional: &[("--areas <a,b>", "")],
        example: "trailmark add loop Q2 --title \"The label on the bag\" --item none --platform docs",
        note: "Pass `--part brief=<file>` now: until a loop has a brief, its bundle is ranked on the title (and its item). The other parts (built, verified, gates, not-verified, lessons) are written by `trailmark close <KEY>`; see `trailmark help close`.",
    },
    TypeHelp {
        name: "review",
        what: "the review of a loop. Id REV-<KEY>, titled \"Review of <KEY>\"; reserves numbers like a loop.",
        required: &[("--for <LOOP-KEY>", "the loop it reviews")],
        optional: &[("--title \"<one line>\"", "default \"Review of <KEY>\"")],
        example: "trailmark add review --for P1",
        note: "Closed by `trailmark close REV-<KEY> --verdict go|go-with-fixes|hold|no-go --gates \"…\" --part …`.",
    },
    TypeHelp {
        name: "fact",
        what: "an operational fact: a trap, a tool's habit, a device — or a recipe (kind recipe: commands to copy). Id FACT-<slug>.",
        required: &[
            ("<slug>", "positional: lowercase words with hyphens, ≤40 chars"),
            ("--title \"<one line>\"", ""),
            ("--field kind=<k>", "env | trap | device | tool | process | state | recipe"),
            ("--field scope=<s>", "repo (committed) | local (docs/records/local/, gitignored)"),
            ("**Why:** and **How to apply:**", "in the body: --part why=<file> --part how=<file>, or a --body file with both"),
        ],
        optional: &[(
            "--field applies_to=<tags>",
            "areas, platforms (web, server, mobile, deploy) or situations (worktree): the bundles it joins",
        )],
        example: "trailmark add fact oven-log --title \"The oven log is written before the door opens\" --field kind=trap --field scope=repo --field applies_to=web --part why=why.md --part how=how.md",
        note: "A recipe is a fact with kind recipe: a body of a few lines — when, the commands in a ``` block, **Why:**, **How to apply:** — under 400 tok4. `trailmark context` prints the recipes matching the task under \"How to run\".",
    },
    TypeHelp {
        name: "deploy",
        what: "a deploy on the author's word. Id DEP-<N> from the positional number.",
        required: &[
            ("<N>", "positional: the deploy number"),
            ("--title \"<one line>\"", ""),
            ("--field target=<t>", "web | mobile | infra"),
            ("--field time=<YYYY-MM-DDTHH:MMZ>", ""),
            ("--field authorized_by=author", "only the author authorizes"),
            ("--field word=\"<what they said>\"", "and --field word_date=<YYYY-MM-DD>"),
            ("--field backup=<what>", "none also needs --field backup_word=\"<what they said>\""),
            ("--field carries=<ids>", "the loops and items it ships"),
            ("--field rollback=<steps>", ""),
            ("commit, binary_sha256, migrations", "when the record has them"),
            ("--part verified=<file> --part not-verified=<file>", ""),
        ],
        optional: &[(
            "--field previous=<DEP-ID>",
            "required once the target has an earlier deploy; filled from the live one when left out",
        )],
        example: "trailmark add deploy 1 --title \"The till ships\" --field target=infra --field time=2026-04-02T12:00Z --field authorized_by=author --field word=\"ship it\" --field word_date=2026-04-02 --field backup=none --field backup_word=\"no backup\" --field carries=LOOP-P1 --field rollback=\"put the previous binary back\" --part verified=verified.md --part not-verified=nv.md",
        note: "A new live deploy supersedes the live one of its target.",
    },
    TypeHelp {
        name: "incident",
        what: "something that went wrong. Id INC-NNNN.",
        required: &[
            ("--title \"<one line>\"", ""),
            ("--severity <s>", "high | medium | low"),
            ("--field production=<true|false>", ""),
            ("--field found_by=<who>", ""),
            ("--part what=<file> --part found=<file>", "## What happened, ## How it was found"),
            ("--part impact=<file> --part changed=<file>", "## Impact, ## What changed"),
        ],
        optional: &[],
        example: "trailmark add incident --title \"The loaf left light\" --severity low --field production=false --field found_by=author --part what=a.md --part found=b.md --part impact=c.md --part changed=d.md",
        note: "",
    },
    TypeHelp {
        name: "charter",
        what: "a standing text. Id <charter prefix>-<key> (default CHARTER-N) from the positional key.",
        required: &[
            ("<N>", "positional: 3 is CHARTER-3 under the default prefix"),
            ("--title \"<one line>\"", ""),
            ("--body <file>", ""),
        ],
        optional: &[],
        example: "trailmark add charter 9 --title \"How the counter works\" --body charter.md",
        note: "",
    },
];

pub fn find(name: &str) -> Option<&'static TypeHelp> {
    TYPES.iter().find(|t| t.name == name)
}

pub fn render(t: &TypeHelp) -> String {
    let width = t
        .required
        .iter()
        .chain(t.optional)
        .map(|(f, _)| f.chars().count())
        .max()
        .unwrap_or(0)
        .min(44);
    let row = |(flag, meaning): &(&str, &str)| {
        if meaning.is_empty() {
            format!("  {flag}\n")
        } else {
            let pad = width.saturating_sub(flag.chars().count());
            format!("  {flag}{}  {meaning}\n", " ".repeat(pad))
        }
    };
    let mut out = format!("trailmark add {}: {}\n\nRequired — a missing field is refused; a missing part is written as `Pending.` (C12 warns),\nand a record is not edited after it is added, so write every part now:\n", t.name, t.what);
    for r in t.required {
        out.push_str(&row(r));
    }
    if !t.optional.is_empty() {
        out.push_str("\nOptional:\n");
        for r in t.optional {
            out.push_str(&row(r));
        }
    }
    out.push_str(&format!("\nExample:\n  {}\n", t.example));
    out.push_str("\n--part and --body take a FILE path, never the text; `--body -` reads stdin.\n");
    if !t.note.is_empty() {
        out.push_str(t.note);
        out.push('\n');
    }
    out
}

/// `trailmark help add`: every type in one line, and how to ask for one.
pub fn overview(flags: &[&str]) -> String {
    let mut out = String::from("trailmark add <type> [KEY] --title \"…\" …   (`trailmark add <type> --help` lists what a type requires)\n\nTypes:\n");
    for t in TYPES {
        let first = t.what.split(". ").next().unwrap_or(t.what);
        out.push_str(&format!("  {:<11} {first}\n", t.name));
    }
    out.push_str(&format!("\nFlags: --{}\n", flags.join(", --")));
    out
}
