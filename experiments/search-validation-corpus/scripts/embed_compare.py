#!/usr/bin/env python3
"""Compare embedding models on the validation corpus, Vector retrieval only.

Every model sees the same units: the non-empty lines of each converted
document (the Unit split of text/plain). A question is embedded once, the
top units are ranked by cosine similarity and folded into documents in
order, and the documents are scored against the gold keys like evaluate.py.
Only the tuning split is scored here; the final split stays held out.

Usage: embed_compare.py --data DIR --config NAME [--config NAME ...] --out results.jsonl
Configs: e5-small, ruri-v3-310m, ruri-v3-310m-title, eg2-768-title, eg2-768-none,
         eg2-256-title
"""

import argparse
import hashlib
import json
import math
import statistics
import time
from collections import defaultdict
from pathlib import Path

import torch
from sentence_transformers import SentenceTransformer

K = 10
UNIT_WINDOW = 400


def split_of(qid: str) -> str:
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
    return units, keys


def configured(name: str):
    dtype = torch.bfloat16
    if name.startswith("eg2"):
        dim = int(name.split("-")[1])
        model = SentenceTransformer(
            "google/embeddinggemma-2", device="cuda", truncate_dim=None if dim == 768 else dim,
            model_kwargs={"torch_dtype": dtype},
            config_kwargs={"vision_config": None, "audio_config": None})
        titled = name.endswith("-title")
        query = lambda q: f"task: search result | query: {q}"
        document = (lambda t, x: f"title: {t} | text: {x}") if titled else (lambda t, x: f"title: none | text: {x}")
    elif name.startswith("ruri"):
        model = SentenceTransformer("cl-nagoya/ruri-v3-310m", device="cuda",
                                    model_kwargs={"torch_dtype": dtype})
        query = lambda q: f"検索クエリ: {q}"
        document = (lambda t, x: f"検索文書: {t}　{x}") if name.endswith("-title") else (lambda t, x: f"検索文書: {x}")
    else:
        model = SentenceTransformer("intfloat/multilingual-e5-small", device="cuda",
                                    model_kwargs={"torch_dtype": dtype})
        query = lambda q: f"query: {q}"
        document = lambda t, x: f"passage: {x}"
    model.max_seq_length = 512
    return model, query, document


def score(gold_sets, ids):
    ranks = [next((i + 1 for i, d in enumerate(ids) if d in s), None) for s in gold_sets]
    found = [r for r in ranks if r is not None]
    return {"r1": sum(1 for r in found if r <= 1) / len(ranks),
            "r10": sum(1 for r in found if r <= K) / len(ranks),
            "rr": 1 / min(found) if found else 0.0,
            "all10": all(r is not None and r <= K for r in ranks)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--config", action="append", required=True)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    units, doc_keys = load_units(args.data)
    key_docs = defaultdict(set)
    for doc, ks in doc_keys.items():
        for k in ks:
            key_docs[k].add(doc)
    questions = []
    for path in sorted((args.data / "questions").glob("stage1-*.jsonl")) + [args.data / "eval/stage1-auto.jsonl"]:
        for line in path.read_text().splitlines():
            if line.strip():
                q = json.loads(line)
                q.setdefault("split", split_of(q["qid"]))
                if q["split"] == "tuning" and q["lane"] == "text" and q["gold"]:
                    questions.append(q)
    print(f"units={len(units)} questions={len(questions)}", flush=True)
    with open(args.out, "a") as out:
        for name in args.config:
            model, query, document = configured(name)
            started = time.time()
            order = sorted(range(len(units)), key=lambda i: len(units[i][2]))
            texts = [document(units[i][1], units[i][2]) for i in order]
            emb = model.encode(texts, batch_size=256, convert_to_tensor=True, normalize_embeddings=True,
                               show_progress_bar=False)
            matrix = torch.empty_like(emb)
            matrix[torch.tensor(order, device=emb.device)] = emb
            embed_s = time.time() - started
            qemb = model.encode([query(q["query"]) for q in questions], convert_to_tensor=True,
                                normalize_embeddings=True, show_progress_bar=False)
            sims = qemb.float() @ matrix.float().T
            top = torch.topk(sims, UNIT_WINDOW, dim=1).indices.tolist()
            per_type = defaultdict(list)
            for q, unit_ids in zip(questions, top):
                ranked = []
                for u in unit_ids:
                    d = units[u][0]
                    if d not in ranked:
                        ranked.append(d)
                    if len(ranked) == K:
                        break
                gold = [key_docs.get(g["key"], set()) for g in q["gold"]]
                if any(not g for g in gold):
                    continue
                s = score(gold, ranked)
                per_type[q["type"]].append(s)
                per_type["ALL"].append(s)
            summary = {t: {"n": len(v), "recall@1": round(statistics.mean(x["r1"] for x in v), 3),
                           "recall@10": round(statistics.mean(x["r10"] for x in v), 3),
                           "mrr": round(statistics.mean(x["rr"] for x in v), 3),
                           "all_in_top10": sum(x["all10"] for x in v)} for t, v in per_type.items()}
            record = {"config": name, "units": len(units), "embed_seconds": round(embed_s, 1),
                      "units_per_second": round(len(units) / embed_s), "by_type": summary}
            out.write(json.dumps(record, ensure_ascii=False) + "\n")
            out.flush()
            print(json.dumps({"config": name, "ALL": summary["ALL"], "embed_s": round(embed_s)},
                             ensure_ascii=False), flush=True)
            del model, emb, matrix
            torch.cuda.empty_cache()


main()
