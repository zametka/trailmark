# How to work

Before any task, run `trailmark context "<task>"` and read that bundle. tok4 is a rough size: the number of characters divided by 4. The bundle is at most 8,000 tok4. If that command says the store is missing, stop. Run `trailmark init` in a new project.

`trailmark terms` is optional. If it prints a term index, you may rewrite the task in those words and ask `trailmark context --terms '<term>, <term>' "<task>"`. Ranking does not depend on it.

Never read a whole document, a generated index, or a directory of records. Use `trailmark show <id>`, `trailmark show <id> --part <name>`, and `trailmark find <words>`.

Record a lesson with `trailmark add rule --for <LOOP-ID> --title "..." --areas <area> --part incident=<file> --part rule=<file>`. Cite the id the command prints. Run `trailmark check` before you finish.

This file is a request, not a guard. Nothing stops a tool from reading the store whole unless a harness hook is installed (`trailmark adapter install claude-code`). Other harnesses have this block only.

Two accepted conventions can disagree. `trailmark show` prints the other one. Follow either, and record the choice with `trailmark link <record> <id> --rel follows`. A later convention that rewrites part of an earlier one uses `--rel narrows`: both stay, and the later one wins where they disagree.
