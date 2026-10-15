#!/usr/bin/env python3
"""Retrieval on JDocQA (public Japanese documents: reports, pamphlets, slides, websites).

The corpus is every annotated page text (`context`) of all three splits; the
questions are the answerable test questions (`no_reason` without 0), and the
gold is the page the annotators found the answer on. A page is also counted
at the PDF level (any page of the gold PDF). JDocQA: CC BY-SA 4.0,
https://github.com/mizuumi/JDocQA (Onami et al., LREC-COLING 2024).

Runs on the GPU machine (torch, sentence-transformers); DIR holds the three
jdocqa_*_all.json annotation files. jdocqa_fuse.py reuses load() and
dense_rank() from here.

Usage: jdocqa_eval.py --data DIR --config NAME [--config NAME ...] --out results.jsonl
Configs: bigram-bm25; hybrid:<dense config> (RRF, k 60, of bigram-bm25 and that
         one); e5-small, ruri-v3-310m, eg2-768-none (EG2_MODEL picks the weights)
"""

import argparse
import json
import math
import os
import statistics
import time
from collections import Counter, defaultdict
from pathlib import Path

K = 10


def load(data: Path, split_for_questions: str = "test"):
    pages, seen = [], {}
    questions = []
    for split in ("train", "validation", "test"):
        for line in (data / f"jdocqa_{split}_all.json").read_text().splitlines():
            if not line.strip():
                continue
            r = json.loads(line)
            text = (r.get("context") or "").strip()
            pid = f"{r['pdf_name']}#{r['question_page_number']}"
            if text and pid not in seen:
                seen[pid] = len(pages)
                pages.append((pid, r["pdf_name"], text))
            if split == split_for_questions and "0" not in str(r["no_reason"]).split(","):
                questions.append({"q": r["original_question"].strip(), "page": pid, "pdf": r["pdf_name"],
                                  "category": r["pdf_category"],
                                  "multi_page": "2" in str(r["no_reason"]).split(",")})
    questions = [q for q in questions if q["page"] in seen]
    return pages, questions


def bigrams(text: str):
    t = "".join(text.split())
    return [t[i:i + 2] for i in range(len(t) - 1)]


def bm25_rank(pages, questions, k1=1.2, b=0.75):
    tfs = [Counter(bigrams(p[2])) for p in pages]
    lens = [sum(tf.values()) for tf in tfs]
    avg = sum(lens) / len(lens)
    postings = defaultdict(list)
    for i, tf in enumerate(tfs):
        for g, c in tf.items():
            postings[g].append((i, c))
    n = len(pages)
    idf = {g: math.log(1 + (n - len(p) + 0.5) / (len(p) + 0.5)) for g, p in postings.items()}
    ranked = []
    for q in questions:
        score = defaultdict(float)
        for g in set(bigrams(q["q"])):
            for i, c in postings.get(g, ()):
                score[i] += idf[g] * c * (k1 + 1) / (c + k1 * (1 - b + b * lens[i] / avg))
        ranked.append([i for i, _ in sorted(score.items(), key=lambda x: -x[1])[:K * 20]])
    return ranked


def configured(name: str):
    import torch
    from sentence_transformers import SentenceTransformer
    dtype = torch.bfloat16
    if name.startswith("eg2"):
        dim = int(name.split("-")[1])
        model = SentenceTransformer(
            os.environ.get("EG2_MODEL", "google/embeddinggemma-2"), device="cuda",
            truncate_dim=None if dim == 768 else dim, trust_remote_code=True,
            model_kwargs={"torch_dtype": dtype},
            config_kwargs={"vision_config": None, "audio_config": None})
        query = lambda q: f"task: search result | query: {q}"
        document = lambda x: f"title: none | text: {x}"
    elif name.startswith("ruri"):
        model = SentenceTransformer("cl-nagoya/ruri-v3-310m", device="cuda", model_kwargs={"torch_dtype": dtype})
        query = lambda q: f"検索クエリ: {q}"
        document = lambda x: f"検索文書: {x}"
    else:
        model = SentenceTransformer("intfloat/multilingual-e5-small", device="cuda", model_kwargs={"torch_dtype": dtype})
        query = lambda q: f"query: {q}"
        document = lambda x: f"passage: {x}"
    model.max_seq_length = 512
    return model, query, document


def dense_rank(name, pages, questions):
    import torch
    model, query, document = configured(name)
    order = sorted(range(len(pages)), key=lambda i: len(pages[i][2]))
    emb = model.encode([document(pages[i][2]) for i in order], batch_size=64, convert_to_tensor=True,
                       normalize_embeddings=True, show_progress_bar=False)
    matrix = torch.empty_like(emb)
    matrix[torch.tensor(order, device=emb.device)] = emb
    qemb = model.encode([query(q["q"]) for q in questions], convert_to_tensor=True,
                        normalize_embeddings=True, show_progress_bar=False)
    top = torch.topk(qemb.float() @ matrix.float().T, K * 20, dim=1).indices.tolist()
    del model, emb, matrix
    torch.cuda.empty_cache()
    return top


def rrf(*lists, k=60):
    score = defaultdict(float)
    for ids in lists:
        for rank, i in enumerate(ids):
            score[i] += 1 / (k + rank + 1)
    return [i for i, _ in sorted(score.items(), key=lambda x: -x[1])[:K * 20]]


def metrics(rows):
    return {"n": len(rows),
            "page_r@1": round(statistics.mean(r["p1"] for r in rows), 3),
            "page_r@10": round(statistics.mean(r["p10"] for r in rows), 3),
            "page_mrr": round(statistics.mean(r["prr"] for r in rows), 3),
            "pdf_r@10": round(statistics.mean(r["d10"] for r in rows), 3),
            "pdf_mrr": round(statistics.mean(r["drr"] for r in rows), 3)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True, type=Path)
    parser.add_argument("--config", action="append", required=True)
    parser.add_argument("--label", default="")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    pages, questions = load(args.data)
    print(json.dumps({"pages": len(pages), "pdfs": len({p[1] for p in pages}), "questions": len(questions)}), flush=True)
    with open(args.out, "a") as out:
        for name in args.config:
            started = time.time()
            if name == "bigram-bm25":
                ranked = bm25_rank(pages, questions)
            elif name.startswith("hybrid:"):
                lexical = bm25_rank(pages, questions)
                dense = dense_rank(name.split(":", 1)[1], pages, questions)
                ranked = [rrf(a, b) for a, b in zip(lexical, dense)]
            else:
                ranked = dense_rank(name, pages, questions)
            seconds = time.time() - started
            groups = defaultdict(list)
            for q, ids in zip(questions, ranked):
                page_ids = [pages[i][0] for i in ids]
                pdfs = []
                for i in ids:
                    if pages[i][1] not in pdfs:
                        pdfs.append(pages[i][1])
                pr = next((n + 1 for n, p in enumerate(page_ids) if p == q["page"]), None)
                dr = next((n + 1 for n, p in enumerate(pdfs) if p == q["pdf"]), None)
                row = {"p1": pr == 1, "p10": pr is not None and pr <= K, "prr": 1 / pr if pr and pr <= K else 0.0,
                       "d10": dr is not None and dr <= K, "drr": 1 / dr if dr and dr <= K else 0.0}
                groups["ALL"].append(row)
                groups[q["category"]].append(row)
                if q["multi_page"]:
                    groups["multi_page"].append(row)
            summary = {g: metrics(v) for g, v in groups.items()}
            record = {"config": name, "label": args.label or os.environ.get("EG2_MODEL", ""),
                      "pages": len(pages), "seconds": round(seconds), "by_group": summary}
            out.write(json.dumps(record, ensure_ascii=False) + "\n")
            out.flush()
            print(json.dumps({"config": name, "label": record["label"], "ALL": summary["ALL"],
                              "seconds": round(seconds)}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
