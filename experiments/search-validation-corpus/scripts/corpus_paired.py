#!/usr/bin/env python3
"""Paired comparisons on the validation corpus from corpus_fuse.py --dump.

Per question the metrics are corpus_fuse.py's (share of gold documents in the
top 10, top-1 share, reciprocal rank). For each pair over the same questions:
the mean difference, a 95% paired bootstrap interval (10,000 resamples, fixed
seed) and an exact two-sided sign test on the questions that differ.

Usage: corpus_paired.py perq.jsonl
"""
import json
import math
import random
import sys
from collections import defaultdict

rows = defaultdict(list)
for line in open(sys.argv[1]):
    r = json.loads(line)
    rows[r["label"]].append(r)

SUBSETS = {
    "final auto+llm": lambda r: r["split"] == "final" and r["source"] in ("stage1-auto.jsonl", "stage1-llm.jsonl"),
    "all answerable": lambda r: True,
}


def sign_test(diff):
    pos = sum(1 for d in diff if d > 0)
    neg = sum(1 for d in diff if d < 0)
    n = pos + neg
    if n == 0:
        return pos, neg, 1.0
    p = 2 * sum(math.comb(n, i) for i in range(min(pos, neg) + 1)) / 2 ** n
    return pos, neg, min(1.0, p)


def compare(label, a, b):
    for subset, keep in SUBSETS.items():
        items = [r for r in rows[label] if keep(r)]
        out = {"pair": f"{label}: {a} - {b}", "subset": subset, "n": len(items)}
        rng = random.Random(20261015)
        for metric, key in (("r@1", "r1"), ("r@10", "r10"), ("mrr", "rr")):
            diff = [r["scores"][a][key] - r["scores"][b][key] for r in items]
            mean = sum(diff) / len(diff)
            boots = sorted(sum(diff[rng.randrange(len(diff))] for _ in diff) / len(diff) for _ in range(10000))
            pos, neg, p = sign_test(diff)
            out[metric] = {"diff": round(mean, 3), "ci95": [round(boots[249], 3), round(boots[9749], 3)],
                           "better_A": pos, "better_B": neg, "sign_p": float(f"{p:.2g}")}
        print(json.dumps(out, ensure_ascii=False))


for label in rows:
    compare(label, "gated_rrf", "concat")
    compare(label, "rrf_jdocqa", "concat")
    compare(label, "gated_rrf", "rrf_jdocqa")
    compare(label, "gated_rrf", "lexical")
