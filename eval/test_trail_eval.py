"""Tests of the evaluation's own arithmetic, run by `cargo test` (tests/eval_harness.rs)."""
import os
import re
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import trail_eval as t  # noqa: E402

BUNDLE = """<!-- trailmark:context target=task budget=8000 tok4=1234 -->
# Context — task
## Most-cited rules
RULE-1  Repetition means my model is wrong
## Your target

Task: fix RULE-99 and DEC-0001
## Gates and traps
FACT-a-trap  A trap
## Rules — one line each (trailmark show <id>)
RULE-5  Silence reads as failure
RULE-7  Another (RULE-8 cited)
## Not included
RULE-6  left out
"""


def runs_for(shown_by_text_on_store):
    """{(text, store): [ids]} -> the runs map `score` reads (form 'lit')."""
    return {
        f"lit|{tk}|{sk}|real": {"shown": ids, "ranked": ids, "lines": ids, "tok4": 100}
        for (tk, sk), ids in shown_by_text_on_store.items()
    }


class Words(unittest.TestCase):
    def test_overlap_counts_distinct_content_stems_of_the_candidate(self):
        self.assertEqual(t.overlap("the worktrees share reservations", "worktree reservation"), 1.0)
        self.assertEqual(t.overlap("the worktrees share reservations", "two checkouts hand out numbers"), 0.0)
        # stop words and short words are not content
        self.assertEqual(t.overlap("a b of the", "the of a"), 0.0)
        self.assertAlmostEqual(t.overlap("ovens and tests", "oven checkouts"), 0.5)

    def test_stem_is_light_and_fixed(self):
        self.assertEqual(t.stem("libraries"), "library")
        self.assertEqual(t.stem("handing"), "hand")
        self.assertEqual(t.stem("class"), "class")
        self.assertEqual(t.stem("its"), "its")


class Bundles(unittest.TestCase):
    def test_the_task_text_and_the_tail_are_not_shown(self):
        b = t.read_bundle(BUNDLE)
        self.assertEqual(b["tok4"], 1234)
        self.assertNotIn("RULE-99", b["shown"])  # the task's own words
        self.assertNotIn("DEC-0001", b["shown"])
        self.assertNotIn("RULE-6", b["shown"])  # "Not included"
        self.assertIn("RULE-1", b["shown"])  # the core counts as shown
        self.assertNotIn("RULE-1", b["ranked"])  # but not as ranked (before the target)
        self.assertEqual(sorted(b["ranked"]), ["FACT-a-trap", "RULE-5", "RULE-7", "RULE-8"])
        self.assertEqual(b["lines"], ["RULE-5", "RULE-7"])


class Measures(unittest.TestCase):
    def setUp(self):
        # Nine briefs A..I, one store each. A needs X: A's bundle shows it, and so do the bundles of
        # B and C on A's store: 2 of 8 wrong texts, above floor(0.125 * 8) = 1, so generic.
        # D needs Y: only D shows it (specific). E needs Z: nobody shows it.
        keys = list("ABCDEFGHI")
        shown = {(tk, sk): [] for tk in keys for sk in keys}
        shown[("A", "A")] = ["X"]
        shown[("B", "A")] = ["X"]
        shown[("C", "A")] = ["X"]
        shown[("D", "D")] = ["Y"]
        self.runs = runs_for(shown)
        need = {"A": ["X"], "D": ["Y"], "E": ["Z"]}
        self.its = [(k, k, "", need.get(k, []), "held") for k in keys]

    def test_specific_generic_and_margin(self):
        pb = t.score(self.runs, self.its, "lit", "held")
        s = t.summarize(pb, boot=200)
        self.assertEqual(s["needs"], 3)
        self.assertEqual(s["shown"], 2)
        self.assertEqual(s["specific"], 1)  # Y
        self.assertEqual(s["generic"], 1)  # X: two of eight wrong texts show it
        self.assertAlmostEqual(s["wrong_mean"], 2 / 8)
        self.assertAlmostEqual(s["margin"], (2 - 2 / 8) / 3)
        lo, hi = s["margin_ci"]
        self.assertLessEqual(lo, s["margin"])
        self.assertGreaterEqual(hi, s["margin"])

    def test_a_ranker_that_shows_everything_everywhere_has_no_margin(self):
        keys = [i[0] for i in self.its]
        shown = {(tk, sk): ["X", "Y", "Z"] for tk in keys for sk in keys}
        s = t.summarize(t.score(runs_for(shown), self.its, "lit", "held"), boot=50)
        self.assertEqual(s["shown"], 3)
        self.assertEqual(s["specific"], 0)
        self.assertAlmostEqual(s["margin"], 0.0)

    def test_paired_difference_counts_gains_and_losses(self):
        a = t.score(self.runs, self.its, "lit", "held")
        keys = [i[0] for i in self.its]
        shown = {(tk, sk): [] for tk in keys for sk in keys}
        shown[("E", "E")] = ["Z"]
        b = t.score(runs_for(shown), self.its, "lit", "held")
        p = t.paired(a, b, boot=50)
        self.assertEqual(p["specific_gained"], 1)  # Z
        self.assertEqual(p["specific_lost"], 1)  # Y
        self.assertAlmostEqual(p["diff"], ((0 - (1 - 2 / 8)) + (0 - 1) + (1 - 0)) / 3)


class Shuffle(unittest.TestCase):
    def test_a_shuffled_store_keeps_ids_and_moves_titles_and_bodies(self):
        with tempfile.TemporaryDirectory() as d:
            old = t.CACHE
            t.CACHE = d
            try:
                rules = os.path.join(t.store_dir("K"), "docs", "records", "rules")
                os.makedirs(rules)
                for i in range(6):
                    with open(os.path.join(rules, f"2.{i}.md"), "w") as f:
                        f.write(f"---\nid: RULE-{i}\ntype: rule\ntitle: Title {i}\n---\nBody {i}\n")
                out = t.shuffle_store("K")
                moved = 0
                for i in range(6):
                    with open(os.path.join(out, "docs", "records", "rules", f"2.{i}.md")) as f:
                        text = f.read()
                    self.assertIn(f"id: RULE-{i}\n", text)
                    j = int(text.split("title: Title ")[1][0])
                    self.assertIn(f"Body {j}", text)  # title and body travel together
                    moved += j != i
                self.assertGreater(moved, 0)
                # the same seed gives the same store
                with open(os.path.join(out, "docs", "records", "rules", "2.0.md")) as f:
                    first = f.read()
                import shutil

                shutil.rmtree(out)
                with open(os.path.join(t.shuffle_store("K"), "docs", "records", "rules", "2.0.md")) as f:
                    self.assertEqual(f.read(), first)
            finally:
                t.CACHE = old


class RandomRanker(unittest.TestCase):
    def test_a_random_ranker_scores_specific_hits_but_no_margin(self):
        # 29 briefs on one store of 400 rules, each bundle shows 40 of them at random. A
        # need is shown about one time in ten and so is rarely shown by more than 3 of 28 wrong
        # texts: random draws are "specific". The margin is what tells it from a ranking.
        with tempfile.TemporaryDirectory() as d:
            old = t.CACHE
            t.CACHE = d
            try:
                rules = os.path.join(t.store_dir("S"), "docs", "records", "rules")
                os.makedirs(rules)
                for i in range(400):
                    with open(os.path.join(rules, f"{i}.md"), "w") as f:
                        f.write(f"---\nid: RULE-{i}\ntype: rule\n---\n")
                keys = [f"B{i}" for i in range(29)]
                runs = {
                    f"lit|{tk}|S|real": {"shown": ["RULE-1"], "ranked": [f"RULE-{i}" for i in range(2, 42)], "lines": [], "tok4": 1}
                    for tk in keys
                }
                its = [(k, "S", "", [f"RULE-{100 + 5 * j + n}" for n in range(5)], "held") for j, k in enumerate(keys)]
                r = t.random_row(runs, its, "lit", "held", draws=40)
                self.assertGreater(r["specific"], 5)  # of 145 needs: about 0.1 x 0.65 x 145
                self.assertLess(abs(r["margin"]), 0.03)
                kept = t.randomize(runs, 1)
                self.assertTrue(all("RULE-1" in v["shown"] and len(v["ranked"]) == 40 for v in kept.values()))
            finally:
                t.CACHE = old


class SyntheticSet(unittest.TestCase):
    def test_the_example_tasks_name_records_the_example_holds(self):
        import json

        root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "examples", "bakery", "docs", "records"))
        ids = set()
        for dirpath, _, files in os.walk(root):
            for name in files:
                if not name.endswith(".md"):
                    continue
                with open(os.path.join(dirpath, name)) as f:
                    m = re.search(r"(?m)^id: (\S+)", f.read())
                if m:
                    ids.add(m.group(1))
        with open(os.path.join(t.DATA, "tasks.json")) as f:
            tasks = json.load(f)["tasks"]
        self.assertGreaterEqual(len(ids), 60)
        self.assertGreaterEqual(len(tasks), 30)
        held = [task for task in tasks if task["split"] == "held"]
        self.assertGreaterEqual(len(held), 20)
        for task in tasks:
            self.assertTrue(task["need"], task["key"])
            for need in task["need"]:
                self.assertIn(need, ids, task["key"])
                self.assertTrue(t.NEED_ID.fullmatch(need), need)


if __name__ == "__main__":
    unittest.main()
