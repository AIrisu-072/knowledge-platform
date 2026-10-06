"""Build the pre-registered MIRACL ja dev public lane (E, 2026-10-06).

Queries with at least one positive judgment are ordered by query ID; the
first 40 calibrate the similarity floor and the next 100 evaluate. The
corpus is the union of every judged passage of those 140 queries. Passage
text stays in the local cache (Apache-2.0 data, never committed); the
repository keeps only IDs and digests in public-ja-lane-manifest.json.

Usage: python3 prepare_public_ja.py CACHE_DIR
"""
import gzip
import hashlib
import json
import pathlib
import sys

CALIBRATION = 40
EVALUATION = 100


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return "sha256:" + h.hexdigest()


def main():
    cache = pathlib.Path(sys.argv[1])
    miracl = cache / "miracl"
    topics = {}
    for line in (miracl / "topics.ja.dev.tsv").read_text(encoding="utf-8").splitlines():
        qid, text = line.split("\t", 1)
        topics[qid] = text
    judgments = {}
    for line in (miracl / "qrels.ja.dev.tsv").read_text(encoding="utf-8").splitlines():
        qid, _, docid, rel = line.split()
        judgments.setdefault(qid, {})[docid] = int(rel)
    eligible = sorted(
        (qid for qid, docs in judgments.items() if qid in topics and any(docs.values())),
        key=lambda qid: (len(qid), qid),
    )
    selected = eligible[: CALIBRATION + EVALUATION]
    if len(selected) != CALIBRATION + EVALUATION:
        sys.exit("not enough judged dev queries")
    needed = {docid for qid in selected for docid in judgments[qid]}
    passages = {}
    shards = sorted((miracl / "corpus").glob("docs-*.jsonl.gz"), key=lambda p: int(p.stem.split("-")[1].split(".")[0]))
    for shard in shards:
        with gzip.open(shard, "rt", encoding="utf-8") as f:
            for line in f:
                row = json.loads(line)
                if row["docid"] in needed:
                    passages[row["docid"]] = {"docid": row["docid"], "title": row["title"], "text": row["text"]}
    missing = needed - passages.keys()
    if missing:
        sys.exit(f"{len(missing)} judged passages missing from the corpus")
    lane = {
        "queries": [
            {"id": qid, "text": topics[qid], "split": "calibration" if i < CALIBRATION else "evaluation"}
            for i, qid in enumerate(selected)
        ],
        "passages": [passages[docid] for docid in sorted(passages)],
        "qrels": [
            {"query": qid, "docid": docid, "relevance": rel}
            for qid in selected
            for docid, rel in sorted(judgments[qid].items())
        ],
    }
    out = cache / "public-ja-lane.json"
    out.write_text(json.dumps(lane, ensure_ascii=False), encoding="utf-8")
    manifest = {
        "lane": "miracl-v1.0-ja-dev",
        "license": "apache-2.0",
        "selection": f"judged-positive dev queries by query ID; 1..{CALIBRATION} calibration, next {EVALUATION} evaluation",
        "source_files": {
            "topics.ja.dev.tsv": sha256(miracl / "topics.ja.dev.tsv"),
            "qrels.ja.dev.tsv": sha256(miracl / "qrels.ja.dev.tsv"),
            **{shard.name: sha256(shard) for shard in shards},
        },
        "lane_file_sha256": sha256(out),
        "query_ids": selected,
        "passage_count": len(passages),
        "positive_judgments": sum(1 for q in selected for r in judgments[q].values() if r > 0),
    }
    print(json.dumps({k: v for k, v in manifest.items() if k not in ("query_ids", "source_files")}))
    (pathlib.Path(__file__).resolve().parent.parent / "public-ja-lane-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
