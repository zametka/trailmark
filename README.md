# trailmark

`trailmark` keeps a project's rules, decisions, loop reports, reviews, deploys and facts as small records with ids. It builds a task-shaped bundle of the records a task needs, checks the store (`trailmark check`), and can install a read guard so an agent asks `trailmark` instead of opening the store as one file.

It is a record book, a bundle, a checker and a guard. It is not a search engine. Finding one rule from a vaguely worded task does not beat a random draw on specific hits. The numbers are under [What it does not do](#what-it-does-not-do).

Version 0.1.0. License: MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`). Copyright 2026 Konstantin Gorodinskii. Source: https://github.com/zametka/trailmark.

## Install

```
cargo install trailmark --locked
```

That puts the `trailmark` binary on the cargo bin path. The crates.io name `trail` is already taken by an unrelated crate, and other programs already install a binary named `trail`.

The adapters are compiled into the binary. A copied binary still installs them. `TRAIL_ADAPTERS=<dir>` reads a folder instead.

`trailmark` finds `trail.toml` by walking up from the working directory. `--root <dir>` and `TRAIL_ROOT` override that.

## Five minutes

In the repository where the records should live:

```
trailmark init
trailmark add loop P1 --title "First slice" --item none --platform docs
trailmark add rule --for P1 --title "Names stay stable" --areas app
trailmark check
trailmark adapter install claude-code
```

`trailmark init` writes `trail.toml`, `docs/records/`, and `AGENTS.md` when that file is absent. It does not replace an existing `trail.toml`. The first rule on an empty store is `RULE-1`. The command prints the id. Cite that id.

`trailmark context` prints a bundle, capped around 8,000 tokens (characters / 4). `trailmark show RULE-12` prints one record. `trailmark check` prints findings. A warning does not fail. An error does.

The source repository includes a filled example, a fictional bakery called Oven, in `examples/bakery`:

```
git clone https://github.com/zametka/trailmark
cd trailmark/examples/bakery
trailmark context "a gluten-free tin sat under wheat flour"
trailmark show RULE-12
trailmark check
```

## Commands

| Command | Example |
| --- | --- |
| `init` | `trailmark init` |
| `add` | `trailmark add rule --for P1 --title "Names stay stable" --areas app` |
| `show` | `trailmark show RULE-1` |
| `find` | `trailmark find gluten rack --type rule` |
| `context` | `trailmark context "the van route changed on the road"` |
| `check` | `trailmark check` |
| `index` | `trailmark index` |
| `terms` | `trailmark terms` |
| `close` | `trailmark close P1 --review owed --gates "trailmark check"` |
| `set` | `trailmark set OPEN-0001 status=closed --why "the scale was replaced" --by LOOP-P1` |
| `link` | `trailmark link LOOP-P1 RULE-1 --rel missed` |
| `tag` | `trailmark tag RULE-1 --add worktree` |
| `retire` | `trailmark retire RULE-1 --why "superseded"` |
| `figure` | `trailmark figure RULE-1 --add "docs/records/figures/scale.png \| the pass scale"` |
| `reserve` | `trailmark reserve rule 10 --for LOOP-P1` |
| `why` | `trailmark why RULE-1` |
| `measure` | `trailmark measure` |
| `adapter` | `trailmark adapter install claude-code` |

`trailmark add <type> --help` lists what that type requires. `trailmark help adapter` says what `--force` replaces. A new project starts with `trailmark init` and `trailmark add`.

## Record types

Ids are minted. The defaults:

| Type | Id | What it is |
| --- | --- | --- |
| rule | `RULE-N` | A lesson, with an incident and a rule |
| charter | `CHARTER-N` | Standing text. `CHARTER-1`, pinned, is the non-negotiables |
| decision | `DEC-0001` | A ruling. `basis` is `word`, `silence`, or `ruling` |
| discussion | `DSC-0001` | Remarks |
| open | `OPEN-0001` | Something owed |
| loop | `LOOP-P1` | The report of a piece of work. Reserves 10 rule numbers and 10 open numbers |
| review | `REV-P1` | The review of that loop |
| item | `ITEM-P1` | A queue entry |
| fact | `FACT-slug` | A trap, a tool, a recipe |
| incident | `INC-0001` | Something that went wrong |
| deploy | `DEP-N` | A deploy. Refused unless a person has given the word |

`[ids]` in `trail.toml` changes the prefixes. `rule_style = "section"` numbers rules `<prefix>-2.N` instead of `<prefix>-N`. Omitted, the table is the defaults above.

A loop's numbers live in `docs/records/reservations/<id>.toml`, and in a ledger under the git common directory so two worktrees do not take the same block. The ledger is not committed.

`index/` is a local cache. `trailmark index` writes it, with its own `.gitignore`. `commit_index = true` commits the index and `trailmark check` refuses a stale file.

## The read guard, and its limits

`trailmark adapter install claude-code` copies hook scripts into `.claude/hooks/` and a skill into `.claude/skills/trailmark/`. When `.claude/settings.json` is absent, the install writes it, with or without `--force`, so the guard is active. When that file already exists, it is never replaced: the block to merge is printed on stderr and the command exits 0. `--force` replaces the copied hook and skill files. Without `--force`, a copied file that differs is left alone and the command exits 7, having written nothing else.

The guard blocks a whole read of a path in `never_whole`, a text file over 100,000 characters, and a git-history read of a removed document. Other reads of the store are logged. Logging is not blocking. Exit 0 lets the tool run. Exit 2, after the tool, tells the agent which `trailmark` command to use.

The guard is a convenience. It is not a security boundary. A person who can run a shell can read the files.

It now logs three shapes that used to pass unseen: `find <store> | xargs cat`, `find <store> -exec cat`, and `bash -c` / `sh -c` whose script reads a record. Still not logged, and accepted as limits:

- a wrapper other than `bash -c` / `sh -c` (`eval`, `env`, `time`, `nice`, `command`, `/bin/cat`)
- a reader not on the list (`sort`, `diff`, `perl`, `ruby`, `node`, `strings`, `base64`, …)
- a write by `echo`, `printf`, or `tee` into a record
- `grep -f <record>`, or a grep with no path
- a tab where a space was expected, or a command past 4,000 characters
- `TRAIL_ALLOW=brief` anywhere in the command text, including an `echo`
- `git diff <rev> -- <removed document>`, which is neither blocked nor logged

A waiver matches a citation by path and by the cited text. If the waived citation is deleted and a new citation of the same text is added in that file, the waiver covers the new one. `trailmark check` does not say so.

## Harnesses

| Harness | What you get |
| --- | --- |
| Claude Code | Hooks and a skill, via `trailmark adapter install claude-code`. The hook is what enforces the guard |
| Codex, Gemini CLI, Grok, Cursor, anything else | `AGENTS.md` only. `trailmark init` writes it when the file is absent. Nothing enforces it |

`trailmark adapter install generic` writes the same block to `.trail/agents-block.md` and does not edit `AGENTS.md`.

## What it does not do

These numbers were measured on one private project, over frozen briefs, against a wrong-brief control, a shuffled store, and a random ranker of the same bundle sizes. No query text and no record text from that project is in this repository. They are not a claim about `examples/bakery`.

`python3 -I eval/trail_eval.py synthetic target/release/trailmark --random 40` is a smoke test of that harness against the bakery records. The tasks are written in the records' own words. The margin it prints is the harness running on its own example. It is not a recall claim, and it is not the measurement below.

Specific means: the needed record is in the bundle, and at most one eighth of the wrong briefs also show it. A random ranker scores specific hits, because a bundle shows a slice of the store and the threshold is about that slice. Its margin is about zero. Margin is the measure that is not fooled by that.

On the held low-overlap briefs, a random ranker scored 4.9 specific hits out of 80, range 2–9. On literal briefs, 5.0 out of 80, range 1–8. On a paraphrase set, 5.8, range 1–11. The word ranking's specific count on that paraphrase set was 4 of 69, below the random row. Its margin on the held low-overlap briefs was −0.004: no content signal above structure and chance. On literal briefs, hits fell from 41 to 31 when titles and bodies were shuffled among records, so a literal brief does find some of what the records say, and a large share of what it shows is structure.

Meaning search (a local embedding model) was measured and is not in this release. Hybrid specific hits were 5 of 80, the random row. Meaning-only was 9 of 80, inside the random range. Its margin over the word ranking on the held low-overlap briefs was +0.100, interval −0.001 to +0.197.

A term index, where an agent rewrites the task into the project's words, was measured and not adopted as part of ranking. Its paired margin over the plain bundle was +0.079 [+0.017, +0.147] on held low-overlap briefs, +0.130 [+0.032, +0.242] on a fresh query set, and +0.133 on a blind need set. A placebo index, every term pointing at another term's records, sat within +0.025 and +0.022 of it, both intervals crossing zero. On literal briefs the real index beat the placebo by +0.086. The words carried the gain. Where the terms pointed did not. `trailmark terms` still ships as an optional aid: a project that writes a term file can ask `trailmark context --terms`. That is not a measured gain.

`trailmark context` still returns a bundle. Use it for the rules a task's words actually share with the store, for the checks, and for the ids code can cite. Do not expect it to retrieve a specific rule from a vague sentence.

## Configuration

`docs/START.md` lists every `trail.toml` key. Unknown keys are an error. `.trail/local.toml` (gitignored) overlays the committed file for paths that differ per machine.

Areas in `trail.toml` are the only areas a record may name. `platform` is separate: `web`, `server`, `mobile`, `infra`, or `docs`.

## Development

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
RUSTFLAGS="--remap-path-prefix=$HOME/.cargo/registry=/cargo/registry --remap-path-prefix=$HOME/.cargo/git=/cargo/git --remap-path-prefix=$HOME/.rustup=/rustup --remap-path-prefix=$PWD=/trailmark" cargo build --release
python3 -I eval/trail_eval.py synthetic target/release/trailmark --random 40
```

The release flags keep a home directory out of the binary. The eval is the synthetic example only. It needs `python3` and the standard library. It checks that the harness runs. It does not measure recall.
