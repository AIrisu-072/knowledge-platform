#!/usr/bin/env python3
"""Fusion of lexical and dense retrieval on JDocQA, with the product's own rules.

Lexical follows the search runtime (search-tantivy, cjk-bigram-v1): text is
NFC-normalized; each CJK run becomes its overlapping bigrams (a single
character stays one token), other alphanumeric runs one lowercased word
(over 40 bytes dropped), anything else separates. Ranking is BM25
(k1 1.2, b 0.75). "Gated" keeps only pages holding at least 60% (rounded up)
of the query's distinct tokens (all of them for two or fewer), pages with the
literal query first, as the runtime does for bigram generations.

Strategies scored on the test questions once, after choosing on validation:
  lexical       BM25 over all pages, no gate
  gated         the runtime's lexical list
  dense         the embedding model alone
  concat        the runtime's PriorityConcat: gated list, then dense hits
  rrf_k60       RRF of lexical and dense, equal weight, k 60
  rrf_tuned     weighted RRF, (w, k) chosen on validation by MRR then r@10
  gated_rrf     the gated list first, then rrf_tuned for the remaining places

The runtime ranks Units (lines), not pages; this ranks whole pages.

Usage: jdocqa_fuse.py --data DIR --dense CONFIG --label NAME --out results.jsonl [--dump perq.jsonl]
"""

import argparse
import json
import math
import statistics
import unicodedata
from collections import Counter, defaultdict
from pathlib import Path

from jdocqa_eval import K, dense_rank, load

DEPTH = 200
WEIGHTS = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]
KS = [0, 1, 2, 3, 5, 10, 20, 60]


def is_cjk(c):
    o = ord(c)
    return (0x3040 <= o <= 0x309F or 0x30A1 <= o <= 0x30FA or 0x30FC <= o <= 0x30FF
            or 0x31F0 <= o <= 0x31FF or 0x3005 <= o <= 0x3007 or 0x3400 <= o <= 0x4DBF
            or 0x4E00 <= o <= 0x9FFF or 0xF900 <= o <= 0xFAFF or 0xFF66 <= o <= 0xFF9F
            or 0x20000 <= o <= 0x2FA1F)


def kind(c):
    return 0 if is_cjk(c) else (1 if c.isalnum() else 2)


def tokens(text):
    text = unicodedata.normalize("NFC", text)
    out, i, n = [], 0, len(text)
    while i < n:
        k = kind(text[i])
        j = i + 1
        while j < n and kind(text[j]) == k:
            j += 1
        run = text[i:j]
        if k == 0:
            out.extend([run] if len(run) == 1 else [run[a:a + 2] for a in range(len(run) - 1)])
        elif k == 1:
            word = run.lower()
            if len(word.encode()) <= 40:
                out.append(word)
        i = j
    return out


class Bm25:
    def __init__(self, pages, k1=1.2, b=0.75):
        self.k1, self.b = k1, b
        self.texts = [unicodedata.normalize("NFC", p[2]) for p in pages]
        tfs = [Counter(tokens(t)) for t in self.texts]
        self.lens = [sum(tf.values()) for tf in tfs]
        self.avg = sum(self.lens) / len(self.lens)
        self.postings = defaultdict(list)
        for i, tf in enumerate(tfs):
            for g, c in tf.items():
                self.postings[g].append((i, c))
        n = len(pages)
        self.idf = {g: math.log(1 + (n - len(p) + 0.5) / (len(p) + 0.5)) for g, p in self.postings.items()}

    def rank(self, query, gated):
        distinct = set(tokens(query))
        score, held = defaultdict(float), Counter()
        for g in distinct:
            for i, c in self.postings.get(g, ()):
                score[i] += self.idf[g] * c * (self.k1 + 1) / (c + self.k1 * (1 - self.b + self.b * self.lens[i] / self.avg))
                held[i] += 1
        if gated and len(distinct) > 1:
            required = len(distinct) if len(distinct) <= 2 else math.ceil(len(distinct) * 3 / 5)
            score = {i: s for i, s in score.items() if held[i] >= required}
        literal = unicodedata.normalize("NFC", query)
        order = sorted(score, key=lambda i: (not (gated and literal in self.texts[i]), -score[i]))
        return order[:DEPTH]


def rrf(lexical, dense, w, k):
    score = defaultdict(float)
    for rank, i in enumerate(lexical):
        score[i] += w / (k + rank + 1)
    for rank, i in enumerate(dense):
        score[i] += (1 - w) / (k + rank + 1)
    return [i for i, _ in sorted(score.items(), key=lambda x: -x[1])[:K]]


def concat(first, second):
    seen, out = set(), []
    for i in first + second:
        if i not in seen:
            seen.add(i)
            out.append(i)
        if len(out) == K:
            break
    return out


def evaluate(pages, questions, ranked):
    rows = defaultdict(list)
    for q, ids in zip(questions, ranked):
        page_ids = [pages[i][0] for i in ids[:K]]
        pr = next((n + 1 for n, p in enumerate(page_ids) if p == q["page"]), None)
        row = (pr == 1, pr is not None, 1 / pr if pr else 0.0)
        rows["ALL"].append(row)
        rows[q["category"]].append(row)
        if q["multi_page"]:
            rows["multi_page"].append(row)
    return {g: {"n": len(v), "r@1": round(statistics.mean(r[0] for r in v), 3),
                "r@10": round(statistics.mean(r[1] for r in v), 3),
                "mrr": round(statistics.mean(r[2] for r in v), 3)} for g, v in rows.items()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--dense", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--dump", type=Path, help="per-question gold ranks on test, one JSON line per question")
    args = parser.parse_args()
    pages, val = load(args.data, "validation")
    _, test = load(args.data, "test")
    bm25 = Bm25(pages)
    lex = {s: [bm25.rank(q["q"], False) for q in qs] for s, qs in (("val", val), ("test", test))}
    gated = {s: [bm25.rank(q["q"], True) for q in qs] for s, qs in (("val", val), ("test", test))}
    dense = {"val": dense_rank(args.dense, pages, val), "test": dense_rank(args.dense, pages, test)}
    grid = []
    for w in WEIGHTS:
        for k in KS:
            m = evaluate(pages, val, [rrf(a, b, w, k) for a, b in zip(lex["val"], dense["val"])])["ALL"]
            grid.append({"w": w, "k": k, **m})
    best = max(grid, key=lambda g: (g["mrr"], g["r@10"]))
    gated_hits = [len(g) for g in gated["test"]]
    test_scores = {
        "lexical": evaluate(pages, test, lex["test"]),
        "gated": evaluate(pages, test, gated["test"]),
        "dense": evaluate(pages, test, dense["test"]),
        "concat": evaluate(pages, test, [concat(a, b) for a, b in zip(gated["test"], dense["test"])]),
        "rrf_k60": evaluate(pages, test, [rrf(a, b, 0.5, 60) for a, b in zip(lex["test"], dense["test"])]),
        "rrf_tuned": evaluate(pages, test, [rrf(a, b, best["w"], best["k"]) for a, b in zip(lex["test"], dense["test"])]),
        "gated_rrf": evaluate(pages, test, [concat(g, rrf(a, b, best["w"], best["k"]))
                                            for g, a, b in zip(gated["test"], lex["test"], dense["test"])]),
    }
    record = {"dense": args.dense, "label": args.label, "validation_questions": len(val),
              "test_questions": len(test), "chosen": best, "grid": grid, "test": test_scores,
              "gated_hits_test": {"zero": sum(1 for h in gated_hits if h == 0),
                                  "under_10": sum(1 for h in gated_hits if h < K),
                                  "median": statistics.median(gated_hits)}}
    with open(args.out, "a") as out:
        out.write(json.dumps(record, ensure_ascii=False) + "\n")
    if args.dump:
        lists = {
            "lexical": lex["test"], "gated": gated["test"], "dense": dense["test"],
            "concat": [concat(a, b) for a, b in zip(gated["test"], dense["test"])],
            "rrf_k60": [rrf(a, b, 0.5, 60) for a, b in zip(lex["test"], dense["test"])],
            "rrf_tuned": [rrf(a, b, best["w"], best["k"]) for a, b in zip(lex["test"], dense["test"])],
            "gated_rrf": [concat(g, rrf(a, b, best["w"], best["k"]))
                          for g, a, b in zip(gated["test"], lex["test"], dense["test"])],
        }
        with open(args.dump, "a") as out:
            for n, q in enumerate(test):
                ranks = {}
                for name, ranked in lists.items():
                    page_ids = [pages[i][0] for i in ranked[n][:K]]
                    ranks[name] = next((r + 1 for r, p in enumerate(page_ids) if p == q["page"]), None)
                out.write(json.dumps({"label": args.label, "n": n, "category": q["category"],
                                      "multi_page": q["multi_page"], "ranks": ranks}) + "\n")
    print(json.dumps({"label": args.label, "chosen": best, "gated_hits_test": record["gated_hits_test"],
                      "test": {k: v["ALL"] for k, v in test_scores.items()}}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
