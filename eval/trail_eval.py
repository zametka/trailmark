#!/usr/bin/env python3
"""Recall evaluation for the synthetic example in examples/bakery.

Stdlib only; run with `python3 -I`.

  python3 -I trail_eval.py synthetic <trailmark-binary> [--random N]

The measures: a need is shown when its id is in the bundle outside the task's own text.
Each task is also run as a wrong text for the others. A shown need is specific when at most
floor(0.125 * wrong) of those wrong texts show it too. The margin is shown (0/1) minus the
share of wrong texts that show it. `--random N` draws a random ranker of the same bundle
sizes. This release does not ship a frozen set from any real project.
A shuffled copy of the example moves titles and bodies among records of one type and keeps
ids, so a hit that survives the shuffle came from structure.
"""
import json
import os
import random
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(HERE, "data")
CACHE = os.environ.get("TRAIL_EVAL_CACHE", os.path.expanduser("~/.cache/trail/eval"))
NEED_ID = re.compile(
    r"\b(RULE-\d+[a-z]?|FACT-[A-Za-z0-9-]*[A-Za-z0-9]|DEC-\d{4}|OPEN-\d{4}|CHARTER-\d+|LOOP-[A-Z]{1,4}\d+[a-z]?(?:\.\d+)?|REV-[A-Z]{1,4}\d+[a-z]?(?:\.\d+)?|ITEM-[A-Z0-9.]+|DSC-\d{4}|INC-\d{4}|DEP-\d+(?:\.\d+)?)\b"
)
SPECIFIC_SHARE = 0.125

STOP = set(
    """a about above after again against all also am an and any are as at be because been before
being below between both but by can could did do does doing done down during each either else
even ever every few for from further get gets got had has have having he her here hers him his
how i if in into is it its itself just let lets like may me might more most much must my no nor
not now of off on once one only or other our ours out over own per same she should so some such
than that the their theirs them then there these they this those through to too under until up
upon us very via was we were what when where whether which while who whom why will with within
without would yet you your yours itself themselves something anything nothing thing things way
ways still really already""".split()
)


# ---------------------------------------------------------------- words


def stem(w):
    """A light, fixed stemmer: plurals, -ed, -ing, -ly. Only for the overlap measure."""
    w = w.lower().strip("'")
    if w.endswith("'s"):
        w = w[:-2]
    for suf, rep, minlen in (
        ("ies", "y", 5),
        ("ing", "", 6),
        ("edly", "", 7),
        ("ed", "", 5),
        ("ly", "", 6),
        ("es", "", 5),
        ("s", "", 4),
    ):
        if w.endswith(suf) and len(w) >= minlen and not (suf == "s" and w.endswith("ss")):
            # "-es" is a plural only after s, x, z, ch, sh ("boxes"); "trees" is "tree" + "s".
            if suf == "es" and not w[:-2].endswith(("s", "x", "z", "ch", "sh")):
                continue
            return w[: len(w) - len(suf)] + rep
    return w


def content_words(text):
    toks = re.findall(r"[a-zA-Z][a-zA-Z']+", text)
    return {stem(t) for t in toks if t.lower() not in STOP and len(t) > 2}


def overlap(original, candidate):
    """The share of the candidate's content words (distinct stems) that the original also uses."""
    c = content_words(candidate)
    if not c:
        return 0.0
    return len(c & content_words(original)) / len(c)


# ---------------------------------------------------------------- stores


def store_dir(key, shuffled=False):
    return os.path.join(CACHE, "stores-shuffled" if shuffled else "stores", key)


def split_record(text):
    """(front matter with its fences, title line index, body) of a record file."""
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---\n", 4)
    if end < 0:
        return None
    return text[: end + 5], text[end + 5 :]


def shuffle_store(key, seed=9):
    """The shuffled-store control: each rule's, decision's and fact's title and body are given to
    another record of its type (ids, links, tags and areas stay). What a bundle still shows of a
    store like this came by structure (links, tags, the core), not by what the records say
    A hit that remains came by structure (links, tags, the core), not by what the records say."""
    src = store_dir(key)
    d = store_dir(key, shuffled=True)
    if os.path.isdir(d):
        return d
    tmp = d + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    shutil.copytree(src, tmp)
    rng = random.Random(f"{seed}:{key}")
    for sub in ("rules", "decisions", "facts"):
        folder = os.path.join(tmp, "docs", "records", sub)
        if not os.path.isdir(folder):
            continue
        names = sorted(n for n in os.listdir(folder) if n.endswith(".md"))
        parts = []
        for n in names:
            with open(os.path.join(folder, n)) as f:
                parts.append(split_record(f.read()))
        ok = [i for i, p in enumerate(parts) if p and re.search(r"^title: ", p[0], re.M)]
        perm = ok[:]
        rng.shuffle(perm)
        for i, j in zip(ok, perm):
            fm_i, _ = parts[i]
            fm_j, body_j = parts[j]
            # The title and the short form travel with the body: they are what the record says.
            said = re.search(r"^title: .*$", fm_j, re.M).group(0)
            short = re.search(r"^short: .*$", fm_j, re.M)
            if short:
                said += "\n" + short.group(0)
            fm = re.sub(r"^short: .*\n", "", fm_i, flags=re.M)
            fm = re.sub(r"^title: .*$", lambda _m: said, fm, count=1, flags=re.M)
            with open(os.path.join(folder, names[i]), "w") as f:
                f.write(fm + body_j)
    os.rename(tmp, d)
    return d


# ---------------------------------------------------------------- bundles


def sections(md):
    """[(heading, text)] of a bundle; the text before the first `## ` is heading ''."""
    out = [("", [])]
    for line in md.splitlines():
        if line.startswith("## "):
            out.append((line[3:], []))
        else:
            out[-1][1].append(line)
    return [(h, "\n".join(t)) for h, t in out]


def read_bundle(md):
    """{shown: ids outside the target, ranked: ids after the target, lines: one-line ids in order, tok4}"""
    m = re.search(r"tok4=(\d+)", md)
    shown, ranked, lines = set(), set(), []
    after = False
    for h, text in sections(md):
        is_target = h in ("Your target", "Brief", "Task")
        if h == "Your target":
            after = True
        if is_target or h.startswith("Not included"):
            continue
        ids = set(NEED_ID.findall(text))
        shown |= ids
        if after:
            ranked |= ids
        if h.startswith("Rules — one line"):
            for line in text.splitlines():
                mm = NEED_ID.match(line)
                if mm:
                    lines.append(mm.group(1))
    return {"shown": sorted(shown), "ranked": sorted(ranked), "lines": lines, "tok4": int(m.group(1)) if m else None}


def run_one(binary, env, store, text, terms=None):
    e = dict(os.environ)
    e.update(env)
    args = [binary, "context"] + (["--terms", terms] if terms is not None else []) + [text]
    p = subprocess.run(args, cwd=store, capture_output=True, text=True, env=e)
    if p.returncode != 0:
        raise RuntimeError(f"{binary} exit {p.returncode} in {store}: {p.stderr[:300]}")
    return p.stdout


# ---------------------------------------------------------------- measures


def score(runs, its, form, split):
    """The measures of one form on one split. `runs` maps '<form>|<text>|<store>|real|shuffled'."""
    chosen = [i for i in its if split == "all" or i[4] == split]
    pool = its  # the wrong texts: every other text of the form, both halves
    per_brief = []
    for tk, sk, _, need, _ in chosen:
        right = runs[f"{form}|{tk}|{sk}|real"]
        wrong = [runs[f"{form}|{o}|{sk}|real"] for o, osk, *_ in pool if o != tk and (not form.startswith("query") or osk == sk)]
        thr = int(SPECIFIC_SHARE * len(wrong))
        rows = []
        for x in need:
            shown = x in right["shown"]
            w = sum(x in r["shown"] for r in wrong)
            rows.append(
                {
                    "id": x,
                    "shown": shown,
                    "wrong_share": w / max(len(wrong), 1),
                    "specific": shown and w <= thr,
                    "generic": shown and w > thr,
                    "line_rank": right["lines"].index(x) + 1 if x in right["lines"] else None,
                }
            )
        sh = runs.get(f"{form}|{tk}|{sk}|shuffled")
        per_brief.append(
            {
                "text": tk,
                "needs": rows,
                "ranked": len(right["ranked"]),
                "ranked_needed": len(set(need) & set(right["ranked"])),
                "lines": len(right["lines"]),
                "tok4": right["tok4"] or 0,
                "shuffled_shown": sum(x in sh["shown"] for x in need) if sh else None,
                "wrong_mean": sum(r["wrong_share"] for r in rows),
            }
        )
    return per_brief


def summarize(per_brief, boot=10000, seed=7):
    needs = [r for b in per_brief for r in b["needs"]]
    n = len(needs)
    shown = sum(r["shown"] for r in needs)
    spec = sum(r["specific"] for r in needs)
    gen = sum(r["generic"] for r in needs)
    wrong = sum(r["wrong_share"] for r in needs)
    margin = (shown - wrong) / n if n else 0.0
    rng = random.Random(seed)
    ms, ss = [], []
    for _ in range(boot):
        sample = [per_brief[rng.randrange(len(per_brief))] for _ in per_brief]
        rows = [r for b in sample for r in b["needs"]]
        if rows:
            ms.append(sum(r["shown"] - r["wrong_share"] for r in rows) / len(rows))
            ss.append(sum(r["specific"] for r in rows) / len(rows))
    ms.sort()
    ss.sort()
    lo, hi = int(0.025 * len(ms)), int(0.975 * len(ms)) - 1
    ranked = sum(b["ranked"] for b in per_brief)
    ranked_needed = sum(b["ranked_needed"] for b in per_brief)
    tok = sum(b["tok4"] for b in per_brief)
    sh = [b["shuffled_shown"] for b in per_brief if b["shuffled_shown"] is not None]
    return {
        "briefs": len(per_brief),
        "needs": n,
        "shown": shown,
        "specific": spec,
        "generic": gen,
        "wrong_mean": wrong,
        "margin": margin,
        "margin_ci": (ms[lo], ms[hi]) if ms else (0, 0),
        "specific_ci": (ss[lo], ss[hi]) if ss else (0, 0),
        "precision": ranked_needed / ranked if ranked else 0.0,
        "ids_per_bundle": ranked / max(len(per_brief), 1),
        "lines_per_bundle": sum(b["lines"] for b in per_brief) / max(len(per_brief), 1),
        "tok4_mean": tok / max(len(per_brief), 1),
        "tok4_per_specific": tok / spec if spec else None,
        "shuffled_shown": sum(sh) if sh else None,
    }


def paired(a, b, boot=10000, seed=11):
    """b minus a, per need (shown - wrong share), the same needs; a bootstrap over briefs."""
    da = {x["text"]: x for x in a}
    diffs = []
    for x in b:
        y = da[x["text"]]
        diffs.append([(r["shown"] - r["wrong_share"]) - (s["shown"] - s["wrong_share"]) for r, s in zip(x["needs"], y["needs"])])
    flat = [d for ds in diffs for d in ds]
    gained = sum(r["specific"] and not s["specific"] for x in b for r, s in zip(x["needs"], da[x["text"]]["needs"]))
    lost = sum(s["specific"] and not r["specific"] for x in b for r, s in zip(x["needs"], da[x["text"]]["needs"]))
    rng = random.Random(seed)
    ms = []
    for _ in range(boot):
        sample = [diffs[rng.randrange(len(diffs))] for _ in diffs]
        rows = [d for ds in sample for d in ds]
        if rows:
            ms.append(sum(rows) / len(rows))
    ms.sort()
    lo, hi = int(0.025 * len(ms)), int(0.975 * len(ms)) - 1
    return {"diff": sum(flat) / len(flat) if flat else 0.0, "ci": (ms[lo], ms[hi]) if ms else (0, 0), "specific_gained": gained, "specific_lost": lost}


def store_ids(store, shuffled=False):
    """The rule, decision and fact ids of a store, from each file's `id:` line."""
    root = os.path.join(store_dir(store, shuffled), "docs", "records")
    out = []
    for sub in ("rules", "decisions", "facts"):
        folder = os.path.join(root, sub)
        if not os.path.isdir(folder):
            continue
        for n in os.listdir(folder):
            if not n.endswith(".md"):
                continue
            with open(os.path.join(folder, n)) as f:
                m = re.search(r"(?m)^id: (\S+)", f.read())
            if m:
                out.append(m.group(1))
    return sorted(out)


def randomize(runs, seed):
    """The random-ranker control: each bundle keeps what it shows before the target (the core, the
    same for every task) and its ranked ids are replaced by as many ids drawn uniformly from its
    store. A ranker that knows nothing still scores specific hits, because a need a random draw
    shows is rarely shown by the other draws, but its margin stays near zero. Read a specific
    count against this row."""
    rng = random.Random(seed)
    pools, out = {}, {}
    for key in sorted(runs):
        r = runs[key]
        _, _, sk, sh = key.split("|")
        if (sk, sh) not in pools:
            pools[(sk, sh)] = store_ids(sk, sh == "shuffled")
        fixed = set(r["shown"]) - set(r["ranked"])
        pool = [x for x in pools[(sk, sh)] if x not in fixed]
        pick = rng.sample(pool, min(len(r["ranked"]), len(pool)))
        out[key] = {"shown": sorted(fixed | set(pick)), "ranked": pick, "lines": [], "tok4": r["tok4"]}
    return out


def random_row(runs, its, form, split, draws=100):
    """Mean specific, its 95% range and mean margin of `draws` random rankers of the same sizes."""
    sp, mg = [], []
    for d in range(draws):
        s = summarize(score(randomize(runs, 1000 + d), its, form, split), boot=1)
        sp.append(s["specific"])
        mg.append(s["margin"])
    sp.sort()
    return {"specific": sum(sp) / draws, "range": (sp[int(0.025 * (draws - 1))], sp[int(0.975 * (draws - 1))]), "margin": sum(mg) / draws}


BAKERY = os.path.abspath(os.path.join(HERE, "..", "examples", "bakery"))


def load_tasks():
    with open(os.path.join(DATA, "tasks.json")) as f:
        return json.load(f)["tasks"]


def synthetic(binary, draws=40):
    """Run the published example. Numbers are for that synthetic store, not for any real project."""
    tasks = load_tasks()
    if len(tasks) < 30:
        sys.exit(f"examples need at least 30 tasks, found {len(tasks)}")
    shutil.rmtree(store_dir("bakery"), ignore_errors=True)
    shutil.rmtree(store_dir("bakery", True), ignore_errors=True)
    os.makedirs(os.path.dirname(store_dir("bakery")), exist_ok=True)
    shutil.copytree(BAKERY, store_dir("bakery"))
    shutil.rmtree(os.path.join(store_dir("bakery"), "docs", "records", "index"), ignore_errors=True)
    shuffle_store("bakery")
    runs = {}
    for t in tasks:
        for sh in (False, True):
            md = run_one(binary, {}, store_dir("bakery", sh), t["text"])
            runs[f"lit|{t['key']}|bakery|{'shuffled' if sh else 'real'}"] = read_bundle(md)
    its = [(t["key"], "bakery", t["text"], t["need"], t["split"]) for t in tasks]
    print("| split | needs | shown | specific | generic | margin [95%] | shuffled shown | random specific | random margin |")
    print("|---|---|---|---|---|---|---|---|---|")
    summary = {}
    for split in ("held", "tune", "all"):
        pb = score(runs, its, "lit", split)
        s = summarize(pb)
        summary[split] = s
        rand = random_row(runs, its, "lit", split, draws) if draws else None
        if rand:
            summary[split]["random_specific"] = rand["specific"]
            summary[split]["random_margin"] = rand["margin"]
        rspec = f"{rand['specific']:.1f}" if rand else ""
        rmar = f"{rand['margin']:+.3f}" if rand else ""
        print(
            f"| {split} | {s['needs']} | {s['shown']} | {s['specific']} | {s['generic']} | "
            f"{s['margin']:+.3f} [{s['margin_ci'][0]:+.3f}, {s['margin_ci'][1]:+.3f}] | "
            f"{s['shuffled_shown']} | {rspec} | {rmar} |"
        )
    print("synthetic example only; examples/bakery. This is a smoke test of the harness, not a recall claim.")
    return summary


def main(argv):
    if not argv:
        sys.exit(__doc__)
    cmd, rest = argv[0], argv[1:]
    if cmd == "synthetic":
        draws = 40
        binary = None
        i = 0
        while i < len(rest):
            if rest[i] == "--random":
                draws = int(rest[i + 1])
                i += 2
            else:
                binary = rest[i]
                i += 1
        if not binary:
            sys.exit("synthetic <trailmark-binary> [--random N]")
        synthetic(os.path.abspath(binary), draws)
    elif cmd == "overlap":
        with open(rest[0]) as a, open(rest[1]) as b:
            print(f"{overlap(a.read(), b.read()):.3f}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
