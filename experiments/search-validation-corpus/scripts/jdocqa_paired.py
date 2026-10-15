#!/usr/bin/env python3
"""Paired comparisons on JDocQA test questions from jdocqa_fuse.py --dump.

Usage: jdocqa_paired.py perq.jsonl

For each pair (A, B) over the same questions: the difference of r@1, r@10 and
MRR, a 95% paired bootstrap interval (10,000 resamples, fixed seed), and an
exact McNemar p-value for r@10 and r@1.
"""
import json
import math
import random
import sys
from collections import defaultdict

rows = defaultdict(dict)
for line in open(sys.argv[1]):
    r = json.loads(line)
    rows[r["label"]][r["n"]] = r["ranks"]


def metric(rank, name):
    if name == "r@1":
        return 1.0 if rank == 1 else 0.0
    if name == "r@10":
        return 1.0 if rank is not None else 0.0
    return 1.0 / rank if rank else 0.0


def mcnemar(a, b):
    only_a = sum(1 for x, y in zip(a, b) if x and not y)
    only_b = sum(1 for x, y in zip(a, b) if y and not x)
    n = only_a + only_b
    if n == 0:
        return only_a, only_b, 1.0
    k = min(only_a, only_b)
    p = 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return only_a, only_b, min(1.0, p)


def compare(title, a_label, a_name, b_label, b_name):
    qs = sorted(set(rows[a_label]) & set(rows[b_label]))
    a = [rows[a_label][q][a_name] for q in qs]
    b = [rows[b_label][q][b_name] for q in qs]
    rng = random.Random(20261015)
    out = {"pair": title, "n": len(qs)}
    for name in ("r@1", "r@10", "mrr"):
        diff = [metric(x, name) - metric(y, name) for x, y in zip(a, b)]
        mean = sum(diff) / len(diff)
        boots = sorted(sum(diff[rng.randrange(len(diff))] for _ in diff) / len(diff) for _ in range(10000))
        out[name] = {"diff": round(mean, 3), "ci95": [round(boots[249], 3), round(boots[9749], 3)]}
    for name in ("r@10", "r@1"):
        only_a, only_b, p = mcnemar([metric(x, name) > 0 for x in a], [metric(y, name) > 0 for y in b])
        out[name]["mcnemar"] = {"only_A": only_a, "only_B": only_b, "p": float(f"{p:.2g}")}
    print(json.dumps(out, ensure_ascii=False))


for label in rows:
    compare(f"{label}: rrf_tuned - lexical", label, "rrf_tuned", label, "lexical")
    compare(f"{label}: rrf_tuned - concat(現行)", label, "rrf_tuned", label, "concat")
    compare(f"{label}: concat(現行) - lexical", label, "concat", label, "lexical")
    compare(f"{label}: rrf_tuned - rrf_k60", label, "rrf_tuned", label, "rrf_k60")
compare("rrf_tuned: eg2-base - ruri", "eg2-base", "rrf_tuned", "ruri-v3-310m", "rrf_tuned")
compare("rrf_tuned: merge-l050 - eg2-base", "merge-Aft-l050", "rrf_tuned", "eg2-base", "rrf_tuned")
compare("dense: merge-l050 - eg2-base", "merge-Aft-l050", "dense", "eg2-base", "dense")
compare("dense: ruri - eg2-base", "ruri-v3-310m", "dense", "eg2-base", "dense")
