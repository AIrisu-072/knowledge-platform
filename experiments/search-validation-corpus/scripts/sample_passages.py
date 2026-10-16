#!/usr/bin/env python3
"""Print short passages to write paraphrase / exception questions from.

Prints `key<TAB>title<TAB>passage` lines: the lead paragraph of sampled
jawiki articles, and law articles that contain an exception (ただし) or a
reference to another law (…法（…）に基づく / に規定する).
"""

import argparse
import json
import random
import re
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--jawiki", type=Path, required=True)
    parser.add_argument("--laws", type=Path, required=True)
    parser.add_argument("--n-jawiki", type=int, default=30)
    parser.add_argument("--n-laws", type=int, default=20)
    parser.add_argument("--seed", type=int, default=11)
    args = parser.parse_args()
    rng = random.Random(args.seed)
    prov = {json.loads(l)["doc"]: json.loads(l) for l in (args.jawiki / "provenance.jsonl").read_text().splitlines()}
    entries = [json.loads(l) for l in (args.jawiki / "documents.jsonl").read_text().splitlines()]
    for entry in rng.sample(entries, args.n_jawiki):
        text = (args.jawiki / entry["file"]).read_text().split("\n")
        lead = next((line for line in text[2:] if len(line) > 80), "")
        print(f"jawiki:{prov[entry['doc']]['page_id']}\t{entry['title']}\t{lead[:260]}")
    lprov = {json.loads(l)["doc"]: json.loads(l) for l in (args.laws / "provenance.jsonl").read_text().splitlines()}
    articles = []
    for line in (args.laws / "documents.jsonl").read_text().splitlines():
        entry = json.loads(line)
        record = lprov[entry["doc"]]
        if record["role"] != "current":
            continue
        for block in (args.laws / entry["file"]).read_text().split("\n\n"):
            if block.startswith("（") and ("ただし" in block or re.search(r"法（[^）]+）", block)) and 80 < len(block) < 700:
                articles.append((f"egov-law:{record['law_id']}", entry["title"], block.replace("\n", " ")))
    for key, title, block in rng.sample(articles, min(args.n_laws, len(articles))):
        print(f"{key}\t{title}\t{block[:420]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
