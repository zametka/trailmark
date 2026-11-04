# Changelog

## 0.1.2 — 2026-11-05

`trailmark check` warns (C23) when no code file matched `[code] include`, or when the files that matched cite no id. A dead id in a folder outside `include` used to pass with only `0 code references` to show for it. `include = []` says a repo has no code to scan, and silences the warning.

A missing figure now says the path is relative to the store, and where it looked. `trailmark init` and the agent instructions show `add rule` with its parts.

README: Five minutes ends at 0 warnings, the commands table runs in order from there, and a test runs both. `why`, `figure`, and `close` examples corrected. The paraphrase comparison now says its denominators differ. Waivers are explained, and a typo that lands on a live id is listed as something `check` does not catch.

## 0.1.1 — 2026-11-04

README badges for the crates.io version, docs.rs, and CI.

## 0.1.0 — 2026-11-04

First release. Records with ids, bundles, `trailmark check`, reservations across worktrees, and harness adapters. The default id scheme is `RULE-N`, `DEC-0001`, `OPEN-0001`. Meaning search is not in this release. The measurements are in the README.
