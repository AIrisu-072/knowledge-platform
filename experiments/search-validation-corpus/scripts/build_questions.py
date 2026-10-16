#!/usr/bin/env python3
"""Generate the automatic part of the evaluation set.

Gold answers are keyed by public, stable source keys (`jawiki:<page_id>`,
`egov-law:<law_id>` for any revision, `egov-rev:<law_revision_id>` for one
revision), never by the opaque corpus ids, so the set is reproducible from
the official sources. Every generated item is `verification: auto` — checked
by construction against the corpus text, not by a person. Items are split
deterministically into `tuning` and `final`.

Types generated here:
  exact_term   a body term that occurs in exactly one document (df = 1)
  title        the document title as the query
  multi_hop    two df = 1 terms from two linked documents; both are needed
  clause       a law article caption + law title -> the law, evidence = article
  no_answer    a term that occurs in no document at all
  as_of        structured lane: the revision in force on a date between two
               enforcement dates (needs dates the search side is not given)

Usage: build_questions.py --corpus DIR [DIR...] --out FILE [--seed 7]
"""

import argparse
import hashlib
import json
import random
import re
from collections import Counter, defaultdict
from datetime import date, timedelta
from pathlib import Path

TERM = re.compile(r"[ァ-ヴー]{4,10}|[一-龥々]{3,6}|[A-Za-z]{6,14}")


def load(corpus_dirs):
    docs = {}
    for directory in corpus_dirs:
        provenance = {}
        for line in (directory / "provenance.jsonl").read_text().splitlines():
            record = json.loads(line)
            provenance[record["doc"]] = record
        for line in (directory / "documents.jsonl").read_text().splitlines():
            entry = json.loads(line)
            record = provenance[entry["doc"]]
            text = (directory / entry["file"]).read_text()
            if record["source"] == "jawiki":
                key = f"jawiki:{record['page_id']}"
                law = None
            else:
                key = f"egov-rev:{record['law_revision_id']}"
                law = f"egov-law:{record['law_id']}"
            docs[key] = {"title": entry["title"], "text": text, "provenance": record, "law": law,
                         "corpus": directory.name, "doc": entry["doc"]}
    return docs


def split_of(qid: str) -> str:
    return "tuning" if int(hashlib.sha256(qid.encode()).hexdigest(), 16) % 10 < 3 else "final"


def gold_for(key, docs):
    """Relevant documents: a law answer accepts any of its revisions."""
    law = docs[key]["law"]
    if law:
        return [{"key": law, "grade": 1}]
    return [{"key": key, "grade": 1}]


def sentence_with(text: str, term: str) -> str:
    position = text.find(term)
    start = max(text.rfind("。", 0, position), text.rfind("\n", 0, position)) + 1
    end_candidates = [i for i in (text.find("。", position), text.find("\n", position)) if i != -1]
    end = min(end_candidates) + 1 if end_candidates else len(text)
    return text[start:end].strip()[:400]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", nargs="+", required=True, type=Path)
    parser.add_argument("--links", type=Path, help="jawiki truth/links.jsonl")
    parser.add_argument("--absent-titles", type=Path, help="titles known to be outside the corpus")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--counts", default="exact_term=25,title=15,multi_hop=15,clause=20,no_answer=15,as_of=10")
    args = parser.parse_args()
    rng = random.Random(args.seed)
    counts = {k: int(v) for k, v in (pair.split("=") for pair in args.counts.split(","))}
    docs = load(args.corpus)
    jawiki = sorted(k for k in docs if k.startswith("jawiki:"))
    laws = sorted(k for k in docs if k.startswith("egov-rev:"))

    # Document frequency of candidate terms over every document.
    df = Counter()
    terms_by_doc = {}
    for key, doc in docs.items():
        terms = Counter(TERM.findall(doc["text"]))
        terms_by_doc[key] = terms
        df.update(set(terms))
    # Law revisions of one law share nearly all text; count df per law instead.
    def df_unique(term, key):
        holders = {docs[k]["law"] or k for k in docs if term in terms_by_doc[k]}
        return holders == {docs[key]["law"] or key}

    questions = []

    def add(kind, query, gold, evidence, lane="text", note=None):
        qid = f"{kind}-{hashlib.sha256((kind + query).encode()).hexdigest()[:10]}"
        questions.append({"qid": qid, "split": split_of(qid), "type": kind, "lane": lane, "query": query,
                          "gold": gold, "evidence": evidence, "verification": "auto", "note": note})

    def salient_term(key):
        title = docs[key]["title"]
        candidates = [t for t, n in terms_by_doc[key].most_common() if n >= 2 and df[t] <= 2
                      and t not in title and title not in t]
        rng.shuffle(candidates)
        for term in candidates:
            if df_unique(term, key):
                return term
        return None

    for key in rng.sample(jawiki, len(jawiki)):
        if sum(q["type"] == "exact_term" for q in questions) >= counts["exact_term"]:
            break
        term = salient_term(key)
        if term:
            add("exact_term", term, gold_for(key, docs), [sentence_with(docs[key]["text"], term)])

    for key in rng.sample(jawiki, counts["title"]):
        add("title", docs[key]["title"], gold_for(key, docs), [docs[key]["title"]])

    if args.links:
        title_to_key = {docs[k]["title"]: k for k in jawiki}
        pairs = []
        for line in args.links.read_text().splitlines():
            record = json.loads(line)
            source = next((k for k in jawiki if docs[k]["doc"] == record["doc"]), None)
            for target in record["targets"]:
                if source and target in title_to_key and title_to_key[target] != source:
                    pairs.append((source, title_to_key[target]))
        rng.shuffle(pairs)
        used = set()
        for source, target in pairs:
            if sum(q["type"] == "multi_hop" for q in questions) >= counts["multi_hop"]:
                break
            if source in used or target in used:
                continue
            a, b = salient_term(source), salient_term(target)
            if a and b:
                used.update({source, target})
                add("multi_hop", f"{a} {b}", gold_for(source, docs) + gold_for(target, docs),
                    [sentence_with(docs[source]["text"], a), sentence_with(docs[target]["text"], b)],
                    note="both documents are required (linked pair)")

    caption = re.compile(r"^（([^）]{2,20})）\n(第[^　\n]+条)", re.M)
    current_laws = [k for k in laws if docs[k]["provenance"]["role"] == "current"]
    clause_items = []
    for key in current_laws:
        for match in caption.finditer(docs[key]["text"]):
            clause_items.append((key, match.group(1), match.group(2)))
    rng.shuffle(clause_items)
    seen_captions = Counter(c for _, c, _ in clause_items)
    for key, cap, article in clause_items:
        if sum(q["type"] == "clause" for q in questions) >= counts["clause"]:
            break
        if seen_captions[cap] > 3:  # captions like （定義） are too generic alone
            continue
        add("clause", f"{docs[key]['title']} {cap}", gold_for(key, docs), [f"（{cap}）{article}"],
            note=f"evidence article {article}")

    if args.absent_titles:
        absent = [t.strip() for t in args.absent_titles.read_text().splitlines() if t.strip()]
        rng.shuffle(absent)
        corpus_text = "\n".join(d["text"] for d in docs.values())
        for title in absent:
            if sum(q["type"] == "no_answer" for q in questions) >= counts["no_answer"]:
                break
            if len(title) >= 3 and title not in corpus_text:
                add("no_answer", title, [], [], note="term occurs in no document")

    previous = [k for k in laws if docs[k]["provenance"]["role"] == "previous"]
    for key in previous[: counts["as_of"]]:
        record = docs[key]["provenance"]
        current = next(k for k in current_laws if docs[k]["law"] == docs[key]["law"])
        start = date.fromisoformat(record["amendment_enforcement_date"])
        end = date.fromisoformat(docs[current]["provenance"]["amendment_enforcement_date"])
        if end - start < timedelta(days=2):
            continue
        as_of = start + (end - start) / 2
        add("as_of", f"{as_of.isoformat()}時点で施行されていた{docs[key]['title']}",
            [{"key": key, "grade": 1}], [], lane="structured",
            note=f"in force {start}..{end}; needs enforcement dates not given to the search side")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w") as out:
        for question in questions:
            out.write(json.dumps(question, ensure_ascii=False) + "\n")
    summary = Counter((q["type"], q["split"]) for q in questions)
    print(json.dumps({f"{t}/{s}": n for (t, s), n in sorted(summary.items())}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
