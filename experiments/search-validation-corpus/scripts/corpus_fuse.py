#!/usr/bin/env python3
"""Lexical, Vector and their fusion on the validation corpus, with the runtime's rules.

The same strategies as jdocqa_fuse.py, on the Units the runtime indexes (the
non-empty lines of each converted document, as embed_compare.py): lexical is
BM25 over Units with the cjk-bigram-v1 analysis, "gated" keeps Units holding
60% of the query's distinct tokens (literal first), and a document ranks by
its best Unit. Vector folds the top 1,024 Units (the runtime's 8x window)
into documents. Nothing is tuned here: RRF uses the (w, k) chosen on JDocQA
validation questions, so the corpus questions stay held out.

Scoring follows evaluate.py: per question, the share of gold documents in the
top 10, the reciprocal rank of the first, and for questions without an
answer a false positive when anything is returned. As a check of the
reproduction, the gated lexical list should land near the runtime's measured
Search (bigram and term share): Recall@10 0.724, MRR 0.743, false positives
0.538 on the final split of the auto and LLM questions.

Strategies: lexical, gated, dense, concat (the runtime's PriorityConcat),
rrf_k60, rrf_jdocqa, and gated_rrf (the gated list first, then rrf_jdocqa).

DIR holds corpus/jawiki-970, corpus/laws-20261006, questions/ and
eval/stage1-auto.jsonl, as for embed_compare.py. Runs on the GPU machine.

Usage: corpus_fuse.py --data DIR --dense CONFIG --label NAME --w W --k K --out results.jsonl
                      [--dump perq.jsonl] [--title]
"""

import argparse
import hashlib
import json
import math
import statistics
import unicodedata
from collections import defaultdict
from pathlib import Path

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from jdocqa_eval import configured
from jdocqa_fuse import K, concat, rrf, tokens

DEPTH = 200
UNIT_WINDOW = 1024


def split_of(qid):
    return "tuning" if int(hashlib.sha256(qid.encode()).hexdigest(), 16) % 10 < 3 else "final"


def load_units(data: Path):
    docs, keys = [], defaultdict(set)
    for name in ("jawiki-970", "laws-20261006"):
        root = data / "corpus" / name
        titles = {}
        for line in (root / "documents.jsonl").read_text().splitlines():
            record = json.loads(line)
            titles[record["doc"]] = (record["title"], root / record["file"])
        for line in (root / "provenance.jsonl").read_text().splitlines():
            record = json.loads(line)
            if record["source"] == "jawiki":
                keys[record["doc"]].add(f"jawiki:{record['page_id']}")
            else:
                keys[record["doc"]].add(f"egov-rev:{record['law_revision_id']}")
                keys[record["doc"]].add(f"egov-law:{record['law_id']}")
        for doc, (title, path) in titles.items():
            docs.append((doc, title, path))
    units = []
    for doc, title, path in docs:
        for line in path.read_text().splitlines():
            if line.strip():
                units.append((doc, title, line.strip()))
    key_docs = defaultdict(set)
    for doc, ks in keys.items():
        for k in ks:
            key_docs[k].add(doc)
    return units, key_docs


def load_questions(data: Path):
    out = []
    for path in sorted((data / "questions").glob("stage1-*.jsonl")) + [data / "eval/stage1-auto.jsonl"]:
        for line in path.read_text().splitlines():
            if line.strip():
                q = json.loads(line)
                q.setdefault("split", split_of(q["qid"]))
                q["source"] = path.name
                if q["lane"] == "text":
                    out.append(q)
    return out


class UnitBm25:
    def __init__(self, units, k1=1.2, b=0.75):
        self.k1, self.b = k1, b
        self.texts = [unicodedata.normalize("NFC", u[2]) for u in units]
        self.docs = [u[0] for u in units]
        vectorizer = CountVectorizer(analyzer=tokens, lowercase=False, dtype=np.float32)
        matrix = vectorizer.fit_transform(self.texts)
        self.lens = np.asarray(matrix.sum(axis=1)).ravel()
        self.avg = float(self.lens.mean())
        self.csc = matrix.tocsc()
        self.vocab = vectorizer.vocabulary_
        n = matrix.shape[0]
        df = np.diff(self.csc.indptr)
        self.idf = np.log(1 + (n - df + 0.5) / (df + 0.5))

    def rank(self, query, gated):
        distinct = set(tokens(query))
        n = len(self.texts)
        score = np.zeros(n, dtype=np.float64)
        held = np.zeros(n, dtype=np.int32)
        for t in distinct:
            col = self.vocab.get(t)
            if col is None:
                continue
            s, e = self.csc.indptr[col], self.csc.indptr[col + 1]
            rows, tf = self.csc.indices[s:e], self.csc.data[s:e]
            score[rows] += self.idf[col] * tf * (self.k1 + 1) / (
                tf + self.k1 * (1 - self.b + self.b * self.lens[rows] / self.avg))
            held[rows] += 1
        cand = np.nonzero(held)[0]
        if gated and len(distinct) > 1:
            required = len(distinct) if len(distinct) <= 2 else math.ceil(len(distinct) * 3 / 5)
            cand = cand[held[cand] >= required]
        literal = unicodedata.normalize("NFC", query)
        is_literal = np.array([gated and literal in self.texts[i] for i in cand], dtype=bool)
        order = cand[np.lexsort((-score[cand], ~is_literal))]
        ranked, seen = [], set()
        for i in order:
            d = self.docs[i]
            if d not in seen:
                seen.add(d)
                ranked.append(d)
                if len(ranked) == DEPTH:
                    break
        return ranked


def titled(name):
    """The model's passage template with the document title (as embed_compare.py's -title configs)."""
    if name.startswith("eg2"):
        return lambda t, x: f"title: {t} | text: {x}"
    if name.startswith("ruri"):
        return lambda t, x: f"検索文書: {t}\u3000{x}"
    raise ValueError(f"no titled template for {name}")


def dense_docs(name, units, questions, with_title=False):
    import torch
    model, query, document = configured(name)
    passage = titled(name) if with_title else (lambda t, x: document(x))
    order = sorted(range(len(units)), key=lambda i: len(units[i][2]))
    emb = model.encode([passage(units[i][1], units[i][2]) for i in order], batch_size=256, convert_to_tensor=True,
                       normalize_embeddings=True, show_progress_bar=False)
    matrix = torch.empty_like(emb)
    matrix[torch.tensor(order, device=emb.device)] = emb
    qemb = model.encode([query(q["query"]) for q in questions], convert_to_tensor=True,
                        normalize_embeddings=True, show_progress_bar=False)
    top = torch.topk(qemb.float() @ matrix.float().T, UNIT_WINDOW, dim=1).indices.tolist()
    del model, emb, matrix
    torch.cuda.empty_cache()
    out = []
    for ids in top:
        ranked, seen = [], set()
        for i in ids:
            d = units[i][0]
            if d not in seen:
                seen.add(d)
                ranked.append(d)
        out.append(ranked[:DEPTH])
    return out


def score(gold_sets, ids):
    ranks = [next((i + 1 for i, d in enumerate(ids[:K]) if d in s), None) for s in gold_sets]
    found = [r for r in ranks if r is not None]
    return {"r1": sum(1 for r in found if r <= 1) / len(ranks), "r10": len(found) / len(ranks),
            "rr": 1 / min(found) if found else 0.0}


def summarize(questions, ranked, key_docs, group_of):
    groups = defaultdict(list)
    negatives = defaultdict(list)
    for q, ids in zip(questions, ranked):
        gold = [key_docs.get(g["key"], set()) for g in q["gold"]]
        if any(not g for g in gold):
            continue
        for g in group_of(q):
            if not gold:
                negatives[g].append(bool(ids))
            else:
                groups[g].append(score(gold, ids))
    out = {}
    for g in sorted(set(groups) | set(negatives)):
        v = groups.get(g, [])
        entry = {"n": len(v)}
        if v:
            entry.update({"recall@1": round(statistics.mean(x["r1"] for x in v), 3),
                          "recall@10": round(statistics.mean(x["r10"] for x in v), 3),
                          "mrr": round(statistics.mean(x["rr"] for x in v), 3)})
        if negatives.get(g):
            entry["no_answer_n"] = len(negatives[g])
            entry["false_positive_rate"] = round(sum(negatives[g]) / len(negatives[g]), 3)
        out[g] = entry
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--dense", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--w", required=True, type=float)
    parser.add_argument("--k", required=True, type=float)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--dump", type=Path, help="per-question scores, one JSON line per question")
    parser.add_argument("--title", action="store_true",
                        help="embed passages with the document title (the runtime uses title: none)")
    args = parser.parse_args()
    units, key_docs = load_units(args.data)
    questions = load_questions(args.data)
    bm25 = UnitBm25(units)
    lexical = [bm25.rank(q["query"], False) for q in questions]
    gated = [bm25.rank(q["query"], True) for q in questions]
    dense = dense_docs(args.dense, units, questions, args.title)
    strategies = {
        "lexical": lexical,
        "gated": gated,
        "dense": dense,
        "concat": [concat(a, b) for a, b in zip(gated, dense)],
        "rrf_k60": [rrf(a, b, 0.5, 60) for a, b in zip(lexical, dense)],
        "rrf_jdocqa": [rrf(a, b, args.w, args.k) for a, b in zip(lexical, dense)],
        "gated_rrf": [concat(g, rrf(a, b, args.w, args.k)) for g, a, b in zip(gated, lexical, dense)],
    }

    def group_of(q):
        groups = [f"{q['split']}/ALL", f"{q['split']}/{q['type']}", "both/ALL"]
        if q["source"] in ("stage1-auto.jsonl", "stage1-llm.jsonl"):
            groups.append(f"{q['split']}/auto+llm")
        return groups

    record = {"dense": args.dense, "label": args.label, "title": args.title, "w": args.w, "k": args.k,
              "units": len(units),
              "results": {name: summarize(questions, ranked, key_docs, group_of)
                          for name, ranked in strategies.items()}}
    with open(args.out, "a") as out:
        out.write(json.dumps(record, ensure_ascii=False) + "\n")
    if args.dump:
        with open(args.dump, "a") as out:
            for n, q in enumerate(questions):
                gold = [key_docs.get(g["key"], set()) for g in q["gold"]]
                if not gold or any(not g for g in gold):
                    continue
                scores = {name: score(gold, ranked[n]) for name, ranked in strategies.items()}
                out.write(json.dumps({"label": args.label, "qid": q["qid"], "split": q["split"],
                                      "type": q["type"], "source": q["source"], "scores": scores},
                                     ensure_ascii=False) + "\n")
    brief = {name: {g: r[g] for g in ("final/auto+llm", "both/ALL") if g in r}
             for name, r in record["results"].items()}
    print(json.dumps({"label": args.label, "units": len(units), "brief": brief}, ensure_ascii=False), flush=True)


main()
