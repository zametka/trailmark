# Starting trailmark, and bringing it into a repo

trailmark is a separate program. A host repo gets two things: `trail.toml` at the root, and a directory of Markdown records. The program does not commit, stage, or push. The command is `trailmark`. The project file stays `trail.toml`, and the git-common ledger and read log stay under `trail/`.

[README.md](../README.md) has the quickstart, the command sequences and the common errors; read it first. This file has the details behind them: every `trail.toml` key, the adapters, and the reference at the end.

## Build

Users install the published binary with `cargo install trailmark --locked`.

From a checkout of this repository:

```
cargo build --release
cargo install --path . --locked
```

The binary is `target/release/trailmark`. `cargo install` puts `trailmark` on your cargo bin path.

The adapter files are built into the binary (`build.rs` embeds `adapters/`), so a copied binary installs them with its source tree gone; `TRAIL_ADAPTERS=<dir>` reads a folder instead. `trailmark adapter install` with no name lists the adapters the build carries.

`trailmark` finds `trail.toml` by walking up from the current directory. `--root <dir>` and `TRAIL_ROOT` override that.

## A new project

### 1. `trail.toml`

Only `store` is required. Name areas after the parts of the repo. A record's `areas` must be one of these names. `platform` is not an area: it is `web`, `server`, `mobile`, `infra`, or `docs`.

```toml
name = "Example"
requires = ">=0.1.0"
store = "docs/records"
cutover = false
frozen = false

[areas]
app = { keywords = ["screen", "page"], paths = ["src/**"] }

[code]
include = ["src/**"]
exclude = ["**/node_modules/**", "**/target/**", "**/dist/**"]
```

Every key `trail.toml` accepts (an unknown one is an error, exit 4, with the nearest known key):

| Key or table | What it sets |
| --- | --- |
| `name`, `requires`, `store` | the project's name, the trailmark version it needs, the store directory (required) |
| `[ids]` | prefixes and `rule_style` (`number` or `section`). Omitted, the tool mints `RULE-1`, `DEC-0001`, `OPEN-0001`, `CHARTER-1` |
| `cutover`, `frozen` | with `cutover`, a manifest that still points at a long source is C18; `frozen` refuses every write |
| `never_whole` | paths an agent must not read whole |
| `commit_index` | `false` (default): `index/` is a local cache with its own `.gitignore`; `true`: committed, and `check` refuses a stale file (C08) |
| `[areas]` | `<name> = { keywords = [...], paths = [...] }`; a rule's `--areas` must name one |
| `[code]` | `include`, `exclude`: the files `check` scans for cited ids. `check` warns (C23) when no file matches or the files that match cite no id. `include = []` says the repo has no code to scan |
| `[budgets]` | `core`, `context_default`, `context_max`, `terms` (the `trailmark terms` print, 3,200), the index budgets, `core_hot_rules`, `fold_rules_area`, `record_warn` |
| `[reserve]` | `rule`, `open`: how many numbers `add loop` reserves (10 each) |
| `[[alias_pattern]]` | `match`, `id` or `range`, `context_only`: how old text cites ids |
| `review_step` | charter id included in a `--review` bundle; follows `superseded_by` |
| `[[measure.task]]` | `name`, `text`, `areas`: the bundles `trailmark measure` sizes |
| `[check]`, `[adapters.<name>]` | `legacy_context_refs`; `memory_md` |

Leave `[[alias_pattern]]` out. Those patterns tell the checker how this repo already cites rules (`§2.10`, `Review of B1`, `#4`). Copying another project's patterns makes `trailmark check` fail on numbers that were never ids.

`never_whole` is a list of paths an agent must not read whole. Put the long documents there. A line ending in `*` is a prefix. A line ending in `/` matches any path that contains it.

Add to `.gitignore`, and do not ignore the store itself. Records are the project's memory. Only machine-local records and the generic adapter's scratch directory stay out of git:

```
docs/records/local/
.trail/
```

### 2. Create the store directory

```
mkdir -p docs/records
```

The directory may be empty. If it is missing, every command exits 4 and says so. `trailmark init` writes `trail.toml` and the store in a new project.

### 3. First records

`--body` and `--part` take a file path. `--body -` reads stdin. A value pasted as `--body` is treated as a path and the command exits 2.

A loop key is 1–4 letters, then digits, then an optional single lowercase letter: `P1`, `B12`. `trailmark add loop P1` writes `LOOP-P1` and reserves the next 10 rule numbers and 10 open numbers. Rules and open items are minted from that block. You do not choose the number yourself. The default id is `RULE-N`. A project that already numbers rules as section `2.N` sets `[ids] rule_style = "section"` and a prefix such as `rule = "GUIDE"`; the minted id is then `GUIDE-2.N`.

```
trailmark add charter 1 --title "How we work" --field pin=true --body charter.md
trailmark add loop P1 --title "First slice" --item none --platform docs
trailmark add rule --for LOOP-P1 --title "Names stay stable" --areas app \
  --part incident=incident.txt --part rule=rule.txt
trailmark index
trailmark check
```

`charter.md` is the body. Lines that start with `- ` in the pinned charter whose key is `1` (`CHARTER-1` by default) are copied into every context bundle, so those lines are the non-negotiables and nothing else. `incident.txt` is one or two sentences. `rule.txt` is the sentence that goes after `> **Rule.**`. The template supplies the heading and the blockquote.

On an empty store the first rule is `RULE-1` and the first open item is `OPEN-0001`, unless `[ids]` says otherwise. The command prints the id. Use that id. An existing store mints the next free number instead.

A `--part` you skip becomes the word `Pending.` The record is still written, `check` warns (C12), and a record is not edited after it is added, so pass every part when you add it. A running loop or review is the exception: `trailmark close` fills its parts.

`trailmark add` exits 6 when the record is invalid, and writes nothing. The stderr line is the checker code. Fix that and run the command again.

Other first-day commands, when you need them:

```
trailmark add decision --title "The address stays" \
  --field decided_by=author --field basis=silence \
  --part decision=decision.txt --part why=why.txt
trailmark add open --for LOOP-P1 --title "Weigh the spare loaf" \
  --areas app --kind follow-up --severity low --owner author \
  --part what=what.txt --part how-to-close=how.txt
```

`basis` is `word`, `silence`, or `ruling`. `word` also needs `--field quote="..."`. Open `kind` is `risk`, `follow-up`, `owed-step`, `unverified`, or `bug`. Open `owner` is `author`, `orchestrator`, or `next-loop`.

Ids the tool mints: `DEC-0001`, `DSC-0001`, `INC-0001` by counting; `FACT-<key>` from the key you pass (`trailmark add fact gate --title "..."`). The whole id is at most 24 characters, or 45 for a `FACT-` id. `DEP-<n>` is a deploy. Do not add a deploy unless a person has said to ship. A deploy without that person's `word` is refused.

### 4. Tell the agent

```
trailmark adapter install generic
```

This copies `agents-block.md` and `trail-start.sh` into `.trail/`. Paste the block at the top of `AGENTS.md` yourself. The command does not edit that file.

On each task the agent runs `trailmark context "<task>"` and reads that bundle. tok4 is characters divided by 4. The bundle is at most 8,000 tok4. Then `trailmark show <id>` and `trailmark find <words>`. It does not read the store directory, `index/`, or a file listed in `never_whole`.

When the work teaches a rule: `trailmark add rule --for LOOP-P1 ...`, cite the printed id in the code, then `trailmark index` and `trailmark check`. A warning is allowed. An error means the store does not match its indexes, or the new record was not written.

There is a second adapter directory beside `generic/`. Its read guard (`hooks/guard-read.sh`, on Read, Grep, Glob, Bash, Edit and Write) blocks three things: a whole read of a `never_whole` path, a text file over 100,000 characters read whole, and a read of a removed document from git history (`git show <rev>:<path>`, `git log -p`, `cat-file`, `checkout`/`restore`, `blame` of a `never_whole` path that is gone; a heredoc's body is text, not a command). Everything else that goes around trailmark — a partial read, a grep or a shell read of the store, an edit of a record by hand — runs, and is logged, one tab-separated line each, to `$(git rev-parse --git-common-dir)/trail/reads.tsv` (time, worktree, session, tool, kind, verdict, target; never committed; past 1 MB it moves to `reads.tsv.1`, replacing the one before), and after the tool the agent is told which command answers it and how to record the miss. Count the log with `cut -f2,5 <that file> | sort | uniq -c`. A command whose brief asks for the read carries `TRAIL_ALLOW=brief` and is logged as `brief`. `trailmark adapter install <that-directory-name> --dry-run` prints the files it would copy, with a diff for any that exist and differ. A missing settings file is created, with or without `--force`. An existing settings file is never replaced: the block to merge is printed on stderr and the command exits 0. `--force` replaces the copied hook and skill files. Do not pass `--force` unless a person has said yes to replacing those files.

What differs per machine (a home directory, a harness's memory folder) goes in `.trail/local.toml`, which overlays `trail.toml` and is gitignored with `.trail/`:

```
[adapters.claude-code]
memory_md = "~/path/to/the/harness/memory"
```

A picture that shows a convention's case goes under the store and on the record: `trailmark figure RULE-1 --add "figures/scale.png | the pass scale"`. The path is relative to the store, and the file has to be there first. `show` and `context` print its path and caption.

## A repo that already has code and documents

Do this when the repo has no records yet. Read an existing `trail.toml` before writing one.

### Start from today

Use the new-project steps.

Write areas and `[code] include` for this repo's directories. Point `never_whole` at the long documents agents currently open whole.

Record a convention the next time work touches it, as its own rule, under the loop that did the work. One lesson is one file. Pasting a whole design document into one rule makes an index that cannot fit its budget.

Old documents stay where they are. They are not the store. New code cites the id `trailmark add` prints.

`trailmark check` reports `C06` when a Markdown file outside the store has a `### 2.N` heading and, before the next heading, an incident line or a `> **Rule.**` block, and that heading is not already the first line of a rule. A numbered section in a design note, with the heading only, is not `C06`. Remove the rule-shaped block, or add the lesson with `trailmark add`.

`cutover = true` turns on `C18`: a manifest whose `source` file is longer than 40 lines is an error. Leave `cutover` false until those sources are short stubs.

## What the agent must not infer

- An id in an example (`RULE-1`, `LOOP-P1`) exists only after a record with that id has been written. Use the id the command prints.
- The tool never commits. A person commits the record files with the code.
- A pair of rules uses one of `supersedes` (the old one is dead), `narrows` (the later one rewrites part of the earlier one; both stay), or `either` (both were accepted; follow either and record the choice with `follows`). "When two conventions meet" below is the definition.
- Exit 0 is ok. Exit 1 is a missing id or a real checker problem. Exit 2 is usage. Exit 3 is a budget. Exit 4 is a missing store or a bad `trail.toml`. Exit 5 is an id conflict or a used-up reservation (`trailmark reserve rule 10 --for LOOP-P1`). Exit 6 is an invalid record and nothing was written. Exit 7 is a precondition, such as a frozen store.

## Reference

Each folder in `adapters/` has an `install.toml` that says which files go where (`[[copy]]`, `[never_whole]`, `[settings]`, `[memory]`); the core reads it and names no harness.

`adapter install` copies files and prints a line diff for any target that exists and differs; it replaces those only with `--force`. `--force` replaces the copied files (the skill, both hook scripts, never-whole.txt, and the memory index when one is configured). A missing settings file is created, with or without `--force`. An existing settings file is never replaced: the block to merge is printed and the command exits 0. Without `--force`, a differing copied file is left alone and the command exits 7, having written nothing else. It does not edit an existing `AGENTS.md`. Paste `agents-block.md` yourself, or let `trailmark init` write `AGENTS.md` when the file is absent. Without a hook, that block is a request. Nothing enforces it.

Every command that writes (`add`, `set`, `link`, `retire`, `figure`, `reserve`, `index`) holds `<store>/.trail.lock` for its whole read-change-write; a second writer waits up to 30 s (`TRAIL_LOCK_WAIT_MS`), then exits 7. A malformed `reservations.toml`, `reservations/<owner>.toml`, `ambiguous.toml` or `waivers.toml` is an error (exit 4), never "nothing in it". Each write goes through its own temp file.

`find --part <name>` searches only that part (`find loaf --part not-verified`). `measure --tasks <file>` reads `[[task]]` tables (`name`, `text`, `areas`). An unknown flag is a usage error (exit 2).

`show` JSON is `{records:[{id,type,title,frontmatter,body,parts}]}`. `find` JSON is `{hits:[{id,type,date,areas,title,score}]}`. `check` JSON is `{errors,warnings,problems:[{path,line,code,severity,message}]}`. `context` JSON is `{markdown,tok4,budget,included,omitted}`. `add` JSON is `{id,path}`.

### When two conventions meet

Three relations, plus the one the tool already had. `trailmark link <from> <to> --rel <name>` writes both sides when the relation has an inverse. `--remove` clears them. A pair uses only one of `supersedes`, `narrows`, and `either`.

- **`supersedes`** retires the whole earlier record. The file stays. `context` and the active indexes skip it. Use it when the earlier convention is dead.
- **`narrows`** points from the later record at the earlier one. Both stay `active`. Where they disagree, the later one wins, and the rest of the earlier one still stands. A decision can narrow a rule: a ruling often rewrites one part of a convention. `show` of the earlier record prints the later rule part (five newest, then a count). `context` carries them as one chunk, so the earlier text is not shown on its own. `why` names the narrowers of a cited id.
- **`either`** means both were accepted and they disagree. Neither wins. A feature may follow either. `show` and `context` print both rule parts and say so. The feature records the choice with **`follows`** (`trailmark link LOOP-P1 RULE-1 --rel follows`). `show` of the feature names the one it followed and the one that was also accepted. `show` of the convention names who followed it.
- **C19** checks that the two sides agree, that `narrows` and `either` stay on active rules or decisions, that a pair does not mix these relations, and that `narrows` does not cycle. A narrower dated before the record it narrows is a warning when both dates are real. More than five narrowers is a warning: restate the rule and supersede it. `unknown` dates skip the date warning.
- **`add`** of a rule or a decision prints a suggestion when another active convention in a shared area uses the same uncommon words. The suggestion is a line to copy. It does not write the edge.
