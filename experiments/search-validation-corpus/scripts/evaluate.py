#!/usr/bin/env python3
"""Score the validation questions against the running Search API host.

Gold keys (`jawiki:<page_id>`, `egov-law:<law_id>`, `egov-rev:<revision_id>`)
are mapped to the published Document Version ids through the corpus
provenance and the ingest manifests; the search side never sees them.

Systems:
  search    POST /v1/search, coverage bodyRequired, top 10 (lexical)
  discover  POST /v1/discover, coverage titleAndPermittedMetadata, the
            qualified Resources in returned order (S1 rank)
  discover-body  the same with coverage bodyRequired (body lexical)

Per question: rank of each gold Resource, Recall@1/5/10, nDCG@10 (binary
gain), reciprocal rank, latency. `no_answer` scores a false positive when any
Resource is returned. `as_of` (structured lane) is reported as not
evaluable: the enforcement dates are not given to the search side. Evidence
location is not evaluable: the API returns no body snippet or locator.

Usage: evaluate.py --questions F [F...] --corpus D [D...] --ingest M [M...]
                   --out results.jsonl [--systems search,discover]
"""

import argparse
import hashlib
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import search_client  # noqa: E402

K = 10


def split_of(qid: str) -> str:
    return "tuning" if int(hashlib.sha256(qid.encode()).hexdigest(), 16) % 10 < 3 else "final"


def key_map(corpus_dirs, ingest_files):
    version_of_doc = {}
    for path in ingest_files:
        for line in Path(path).read_text().splitlines():
            record = json.loads(line)
            if record.get("status") == "published":
                version_of_doc[record["doc"]] = record["versionId"]
    keys = defaultdict(set)
    for directory in corpus_dirs:
        for line in (Path(directory) / "provenance.jsonl").read_text().splitlines():
            record = json.loads(line)
            version = version_of_doc.get(record["doc"])
            if not version:
                continue
            if record["source"] == "jawiki":
                keys[f"jawiki:{record['page_id']}"].add(version)
            else:
                keys[f"egov-rev:{record['law_revision_id']}"].add(version)
                keys[f"egov-law:{record['law_id']}"].add(version)
    return keys


def ranked(system: str, query: str, actor: str):
    if system == "search":
        status, body, ms = search_client.search(actor, query, "bodyRequired", K)
        ids = [item["resourceId"] for item in body.get("items", [])] if status == 200 else []
    else:
        coverage = "bodyRequired" if system == "discover-body" else "titleAndPermittedMetadata"
        status, body, ms = search_client.discover(actor, query, coverage)
        ids = [item["resourceId"] for item in body.get("qualifiedResources", [])] if status == 200 else []
    return status, ids, ms


def score(gold_sets, ids):
    """gold_sets: one set of acceptable version ids per required answer."""
    ranks = []
    for acceptable in gold_sets:
        rank = next((i + 1 for i, rid in enumerate(ids) if rid in acceptable), None)
        ranks.append(rank)
    found = [r for r in ranks if r is not None]
    recall = {k: sum(1 for r in ranks if r is not None and r <= k) / len(ranks) for k in (1, 5, 10)}
    dcg = sum(1 / math.log2(r + 1) for r in found if r <= K)
    ideal = sum(1 / math.log2(i + 2) for i in range(min(len(ranks), K)))
    return {"ranks": ranks, "recall": recall, "ndcg10": dcg / ideal if ideal else 0.0,
            "rr": 1 / min(found) if found else 0.0, "all_in_top10": all(r is not None and r <= K for r in ranks)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--questions", nargs="+", required=True)
    parser.add_argument("--corpus", nargs="+", required=True)
    parser.add_argument("--ingest", nargs="+", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--systems", default="search,discover")
    parser.add_argument("--actor", default="poc-human")
    parser.add_argument("--split", choices=["tuning", "final"], help="score only one split")
    args = parser.parse_args()
    keys = key_map(args.corpus, args.ingest)
    questions = []
    for path in args.questions:
        questions += [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]
    results = []
    for question in questions:
        question.setdefault("split", split_of(question["qid"]))
        if args.split and question["split"] != args.split:
            continue
        for system in args.systems.split(","):
            record = {"qid": question["qid"], "type": question["type"], "split": question["split"],
                      "lane": question["lane"], "verification": question["verification"], "system": system}
            if question["lane"] == "structured":
                record["outcome"] = "not_evaluable"
                record["reason"] = "enforcement dates are not given to the search side"
                results.append(record)
                continue
            gold_sets = [keys.get(g["key"], set()) for g in question["gold"]]
            if any(not s for s in gold_sets):
                record["outcome"] = "not_evaluable"
                record["reason"] = "gold document was not ingested"
                results.append(record)
                continue
            status, ids, ms = ranked(system, question["query"], args.actor)
            record.update({"status": status, "ms": round(ms, 1), "returned": len(ids)})
            if status != 200:
                record["outcome"] = "error"
            elif not gold_sets:
                record["outcome"] = "false_positive" if ids else "correct_empty"
            else:
                record.update(score(gold_sets, ids))
                record["outcome"] = "scored"
            results.append(record)
    args.out.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in results))
    print(json.dumps(summarize(results), ensure_ascii=False, indent=1))
    return 0


def summarize(results):
    summary = {}
    groups = defaultdict(list)
    for r in results:
        groups[(r["system"], r["split"], r["type"])].append(r)
        groups[(r["system"], r["split"], "ALL")].append(r)
    for (system, split, kind), items in sorted(groups.items()):
        scored = [r for r in items if r["outcome"] == "scored"]
        negatives = [r for r in items if r["outcome"] in ("false_positive", "correct_empty")]
        entry = {"n": len(items), "scored": len(scored),
                 "not_evaluable": sum(r["outcome"] == "not_evaluable" for r in items),
                 "errors": sum(r["outcome"] == "error" for r in items)}
        if scored:
            entry.update({
                "recall@1": round(statistics.mean(r["recall"][1] for r in scored), 3),
                "recall@5": round(statistics.mean(r["recall"][5] for r in scored), 3),
                "recall@10": round(statistics.mean(r["recall"][10] for r in scored), 3),
                "ndcg@10": round(statistics.mean(r["ndcg10"] for r in scored), 3),
                "mrr": round(statistics.mean(r["rr"] for r in scored), 3),
                "missed_top10": sum(1 for r in scored if not r["all_in_top10"]),
            })
        if negatives:
            entry["false_positive_rate"] = round(
                sum(r["outcome"] == "false_positive" for r in negatives) / len(negatives), 3)
        latencies = sorted(r["ms"] for r in items if "ms" in r)
        if latencies:
            entry["p50_ms"] = latencies[len(latencies) // 2]
            entry["p95_ms"] = latencies[min(len(latencies) - 1, int(len(latencies) * 0.95))]
        summary[f"{system}/{split}/{kind}"] = entry
    return summary


if __name__ == "__main__":
    raise SystemExit(main())
