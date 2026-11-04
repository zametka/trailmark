# trailmark

Before any task, run `trailmark context "<task>"` and work from that bundle. If that command says the store is missing, stop. Run `trailmark init` in a new project.

`trailmark terms` is optional. If it prints a term index, you may rewrite the task in those words and ask `trailmark context --terms '<term>, <term>' "<task>"`. Ranking does not depend on it.

Do not read the store, a generated index, or a legacy source whole. Ask `trailmark show <id>` for one record, `trailmark show <id> --part <name>` for one part, and `trailmark find <words>` to search.

Record a lesson with `trailmark add rule --for <LOOP-ID> --title "..." --areas <area>`. Cite the id the command prints. `trailmark add <type> --help` lists what a type requires.

The bundle's "How to run" holds recipes: commands to copy. A command you needed and did not find is a recipe: `trailmark add fact recipe-<slug> --field kind=recipe --field scope=repo --field applies_to=<words a task would use> --body <file>`. A rule you needed that the bundle left out: `trailmark link <LOOP-ID> <rule id> --rel missed`, and `trailmark tag <rule id> --add <the word your task used>`.

Two accepted conventions can disagree. `trailmark show` prints the other one. Follow either, and record the choice with `trailmark link <record> <id> --rel follows`. A later convention that rewrites part of an earlier one uses `--rel narrows`: both stay, and the later one wins where they disagree.

`trailmark check` before you finish. A warning is allowed. An error means the record was not written, or the store does not match its indexes.
