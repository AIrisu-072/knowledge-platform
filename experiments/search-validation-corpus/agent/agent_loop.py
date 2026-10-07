#!/usr/bin/env python3
"""Autonomous retrieval loop of a local LLM over the validation Search API.

The LLM (OpenAI-compatible endpoint, e.g. Bonsai2 behind llama-swap) sees only
tools: keyword search (Search API, body lexical), semantic search (Discover,
Vector + lexical), reading a document by id, a notebook, and a final answer.
It may call them as many times as the budget allows. Old tool outputs are
shortened in the history so that hundreds of calls fit in the context.

Reading a document returns passages of the original that was uploaded. The
experiment reads the same bytes from the local corpus copy (the Document API
download path is checked separately).

Scoring (per question, tuning split unless --split final):
  cited documents  answer.documents in order -> Recall@1/5/10, MRR
  evidence         each quoted passage is verbatim in the cited document
  no_answer        a false positive when documents are cited
  cost             tool calls, LLM turns, prompt/completion tokens, seconds

Usage: agent_loop.py --variant NAME --system PROMPT.md --tools TOOLS.json
                     [--skill SKILL.md] --questions F [F...] --corpus D [D...]
                     --ingest M [M...] --out DIR [--budget 60] [--workers 4]
"""

import argparse
import concurrent.futures
import json
import os
import re
import statistics
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "scripts"))
import evaluate  # noqa: E402
import search_client  # noqa: E402

KEEP_FULL_RESULTS = 6
SNIPPET_CHARS = 160
READ_CHARS = 1800


class Corpus:
    def __init__(self, corpus_dirs, ingest_files):
        doc_of_version = {}
        for path in ingest_files:
            for line in Path(path).read_text().splitlines():
                record = json.loads(line)
                if record.get("status") == "published":
                    doc_of_version[record["versionId"]] = record["doc"]
        file_of_doc = {}
        for directory in corpus_dirs:
            for line in (Path(directory) / "documents.jsonl").read_text().splitlines():
                record = json.loads(line)
                file_of_doc[record["doc"]] = Path(directory) / record["file"]
        self.file_of_version = {v: file_of_doc[d] for v, d in doc_of_version.items() if d in file_of_doc}
        self.cache = {}

    def text(self, version_id):
        if version_id not in self.cache:
            path = self.file_of_version.get(version_id)
            self.cache[version_id] = path.read_text() if path else None
        return self.cache[version_id]


def bigrams(text):
    text = re.sub(r"\s+", "", text)
    return {text[i:i + 2] for i in range(len(text) - 1)}


class Tools:
    def __init__(self, corpus, actor):
        self.corpus = corpus
        self.actor = actor
        self.notes = []
        self.seen = set()

    def call(self, name, args):
        if name == "search_keyword":
            return self.search_keyword(args.get("query", ""), int(args.get("limit") or 10))
        if name == "search_semantic":
            return self.search_semantic(args.get("query", ""))
        if name == "read_document":
            return self.read_document(args.get("id", ""), args.get("query"), int(args.get("page") or 0))
        if name == "note":
            self.notes.append(str(args.get("text", ""))[:500])
            return {"saved": len(self.notes)}
        return {"error": f"unknown tool {name}"}

    def _items(self, items):
        out = []
        for item in items:
            rid = item["resourceId"]
            entry = {"id": rid, "title": item.get("title")}
            snippet = (item.get("snippet") or {}).get("text")
            if snippet:
                entry["snippet"] = snippet[:SNIPPET_CHARS]
            if rid in self.seen:
                entry["seen"] = True
            self.seen.add(rid)
            out.append(entry)
        return out

    def search_keyword(self, query, limit):
        status, body, _ = search_client.search(self.actor, query, "bodyRequired", max(1, min(limit, 20)))
        if status != 200:
            return {"error": f"search failed ({status})"}
        return {"results": self._items(body.get("items", []))}

    def search_semantic(self, query):
        status, body, _ = search_client.discover(self.actor, query, "bodyRequired")
        if status != 200:
            return {"error": f"search failed ({status})"}
        return {"results": self._items(body.get("qualifiedResources", [])[:10])}

    def read_document(self, rid, query, page):
        text = self.corpus.text(rid)
        if text is None:
            return {"error": "unknown document id"}
        if not query:
            start = page * READ_CHARS
            return {"id": rid, "page": page, "pages": (len(text) + READ_CHARS - 1) // READ_CHARS,
                    "text": text[start:start + READ_CHARS]}
        wanted = bigrams(query)
        lines = text.splitlines()
        scored = []
        for number, line in enumerate(lines):
            if not line.strip():
                continue
            overlap = len(wanted & bigrams(line))
            if overlap:
                scored.append((overlap / max(1, len(wanted)), number))
        scored.sort(reverse=True)
        passages, used, taken = [], 0, set()
        for _, number in scored:
            if number in taken:
                continue
            window = range(max(0, number - 1), min(len(lines), number + 2))
            chunk = "\n".join(lines[i] for i in window if i not in taken)
            taken.update(window)
            if used + len(chunk) > READ_CHARS:
                break
            passages.append({"line": number + 1, "text": chunk})
            used += len(chunk)
        return {"id": rid, "passages": passages, "lines": len(lines)}


def llm(endpoint, model, messages, tools, max_tokens):
    payload = {"model": model, "messages": messages, "tools": tools, "max_tokens": max_tokens,
               "temperature": 0.6, "top_p": 0.95}
    request = urllib.request.Request(f"{endpoint}/v1/chat/completions", data=json.dumps(payload).encode(),
                                     method="POST", headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=600) as response:
        return json.loads(response.read())


def shorten(message):
    try:
        body = json.loads(message["content"])
    except (ValueError, TypeError):
        return
    if "results" in body:
        body = {"results": [{"id": r["id"], "title": r.get("title")} for r in body["results"]], "shortened": True}
    elif "passages" in body or "text" in body:
        body = {"id": body.get("id"), "shortened": "本文は省略済み。必要なら再度read_documentで読む"}
    else:
        return
    message["content"] = json.dumps(body, ensure_ascii=False)


ANSWER_TOOL = {"type": "function", "function": {
    "name": "answer",
    "description": "調査を終えて回答する。これを呼ぶと終了する。",
    "parameters": {"type": "object", "properties": {
        "no_answer": {"type": "boolean", "description": "文書群に答えが無いと判断したらtrue"},
        "documents": {"type": "array", "items": {"type": "string"},
                      "description": "答えの根拠となる文書のid。関連が強い順。最大10件"},
        "evidence": {"type": "array", "items": {"type": "object", "properties": {
            "id": {"type": "string"}, "quote": {"type": "string", "description": "文書からそのまま引用した根拠の文"}},
            "required": ["id", "quote"]}},
        "answer": {"type": "string", "description": "質問への短い回答"}},
        "required": ["no_answer", "documents", "evidence", "answer"]}}}


def run_question(question, args, corpus, system_prompt, tools):
    toolbox = Tools(corpus, args.actor)
    messages = [{"role": "system", "content": system_prompt},
                {"role": "user", "content": question["query"]}]
    trace = {"qid": question["qid"], "calls": [], "turns": 0, "prompt_tokens": 0, "completion_tokens": 0}
    started = time.perf_counter()
    final = None
    tool_results = []
    while trace["turns"] < args.budget + 5:
        trace["turns"] += 1
        remaining = args.budget - len(trace["calls"])
        if remaining <= 0:
            messages.append({"role": "user", "content": "検索の予算を使い切った。answerを呼んで回答すること。"})
        try:
            response = llm(args.endpoint, args.model, messages, tools, args.max_tokens)
        except Exception as error:  # noqa: BLE001
            trace["error"] = f"llm: {error}"
            break
        usage = response.get("usage") or {}
        trace["prompt_tokens"] += usage.get("prompt_tokens", 0)
        trace["completion_tokens"] += usage.get("completion_tokens", 0)
        message = response["choices"][0]["message"]
        calls = message.get("tool_calls") or []
        messages.append({"role": "assistant", "content": message.get("content") or "", "tool_calls": calls} if calls
                        else {"role": "assistant", "content": message.get("content") or ""})
        if not calls:
            messages.append({"role": "user", "content": "ツールで調べるか、answerを呼んで終了すること。"})
            continue
        for call in calls:
            name = call["function"]["name"]
            try:
                arguments = json.loads(call["function"].get("arguments") or "{}")
            except ValueError:
                arguments = {}
                result = {"error": "arguments must be JSON"}
            if name == "answer":
                final = arguments
                break
            tool_started = time.perf_counter()
            result = toolbox.call(name, arguments)
            trace["calls"].append({"tool": name, "args": arguments,
                                   "ms": round((time.perf_counter() - tool_started) * 1000, 1),
                                   "n": len(result.get("results", result.get("passages", [])) or [])})
            content = {"role": "tool", "tool_call_id": call["id"], "content": json.dumps(result, ensure_ascii=False)}
            messages.append(content)
            tool_results.append(content)
            if len(tool_results) > KEEP_FULL_RESULTS:
                shorten(tool_results[-KEEP_FULL_RESULTS - 1])
        if final is not None:
            break
    trace["seconds"] = round(time.perf_counter() - started, 1)
    trace["final"] = final
    trace["notes"] = toolbox.notes
    return trace


def score(question, trace, keys, corpus):
    record = {"qid": question["qid"], "type": question["type"], "split": question["split"],
              "verification": question["verification"], "tool_calls": len(trace["calls"]),
              "turns": trace["turns"], "seconds": trace["seconds"],
              "prompt_tokens": trace["prompt_tokens"], "completion_tokens": trace["completion_tokens"]}
    final = trace.get("final")
    if final is None:
        record["outcome"] = "no_final"
        return record
    documents = [d for d in final.get("documents") or [] if isinstance(d, str)][:10]
    evidence = [e for e in final.get("evidence") or [] if isinstance(e, dict)]
    verbatim = [bool(corpus.text(e.get("id")) and e.get("quote") and e["quote"].strip()
                     and e["quote"].strip() in corpus.text(e["id"])) for e in evidence]
    record["evidence_verbatim"] = round(sum(verbatim) / len(verbatim), 3) if verbatim else None
    record["declared_no_answer"] = bool(final.get("no_answer"))
    if not question["gold"]:
        record["outcome"] = "false_positive" if documents and not final.get("no_answer") else "correct_empty"
        return record
    gold_sets = [keys.get(g["key"], set()) for g in question["gold"]]
    if any(not s for s in gold_sets):
        record["outcome"] = "not_evaluable"
        return record
    record.update(evaluate.score(gold_sets, documents))
    record["outcome"] = "scored"
    return record


def summarize(records):
    out = {}
    groups = {}
    for r in records:
        groups.setdefault(r["type"], []).append(r)
        groups.setdefault("ALL", []).append(r)
    for kind, items in sorted(groups.items()):
        scored = [r for r in items if r["outcome"] == "scored"]
        negatives = [r for r in items if r["outcome"] in ("false_positive", "correct_empty")]
        entry = {"n": len(items), "no_final": sum(r["outcome"] == "no_final" for r in items)}
        if scored:
            entry["recall@10"] = round(statistics.mean(r["recall"][10] for r in scored), 3)
            entry["recall@1"] = round(statistics.mean(r["recall"][1] for r in scored), 3)
            entry["mrr"] = round(statistics.mean(r["rr"] for r in scored), 3)
        if negatives:
            entry["false_positive_rate"] = round(
                sum(r["outcome"] == "false_positive" for r in negatives) / len(negatives), 3)
        verbatim = [r["evidence_verbatim"] for r in items if r.get("evidence_verbatim") is not None]
        if verbatim:
            entry["evidence_verbatim"] = round(statistics.mean(verbatim), 3)
        entry["tool_calls_mean"] = round(statistics.mean(r["tool_calls"] for r in items), 1)
        entry["tool_calls_max"] = max(r["tool_calls"] for r in items)
        entry["seconds_p50"] = sorted(r["seconds"] for r in items)[len(items) // 2]
        entry["prompt_tokens_mean"] = round(statistics.mean(r["prompt_tokens"] for r in items))
        out[kind] = entry
    return out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--variant", required=True)
    parser.add_argument("--system", required=True, type=Path)
    parser.add_argument("--tools", required=True, type=Path)
    parser.add_argument("--skill", type=Path)
    parser.add_argument("--questions", nargs="+", required=True)
    parser.add_argument("--corpus", nargs="+", required=True)
    parser.add_argument("--ingest", nargs="+", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--endpoint", default=os.environ.get("AGENT_LLM_ENDPOINT"), help="OpenAI-compatible base URL")
    parser.add_argument("--model", default="bonsai-2-27b")
    parser.add_argument("--actor", default="poc-human")
    parser.add_argument("--budget", type=int, default=60)
    parser.add_argument("--max-tokens", type=int, default=4096)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--split", choices=["tuning", "final"], default="tuning")
    parser.add_argument("--types", help="comma-separated question types to run")
    parser.add_argument("--limit", type=int)
    args = parser.parse_args()

    system_prompt = args.system.read_text()
    if args.skill:
        system_prompt += "\n\n" + args.skill.read_text()
    tools = json.loads(args.tools.read_text()) + [ANSWER_TOOL]
    keys = evaluate.key_map(args.corpus, args.ingest)
    corpus = Corpus(args.corpus, args.ingest)
    questions = []
    for path in args.questions:
        questions += [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]
    selected = []
    for question in questions:
        question.setdefault("split", evaluate.split_of(question["qid"]))
        if question["split"] != args.split or question["lane"] != "text":
            continue
        if args.types and question["type"] not in args.types.split(","):
            continue
        selected.append(question)
    if args.limit:
        selected = selected[:args.limit]

    out = args.out / args.variant
    out.mkdir(parents=True, exist_ok=True)
    traces, records = [], []
    with concurrent.futures.ThreadPoolExecutor(args.workers) as pool:
        futures = {pool.submit(run_question, q, args, corpus, system_prompt, tools): q for q in selected}
        for future in concurrent.futures.as_completed(futures):
            question = futures[future]
            trace = future.result()
            traces.append(trace)
            records.append(score(question, trace, keys, corpus))
            print(f"{question['qid']} {records[-1]['outcome']} calls={len(trace['calls'])} {trace['seconds']}s",
                  file=sys.stderr, flush=True)
    (out / "traces.jsonl").write_text("".join(json.dumps(t, ensure_ascii=False) + "\n" for t in traces))
    (out / "scores.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records))
    summary = {"variant": args.variant, "model": args.model, "budget": args.budget, "split": args.split,
               "system": args.system.name, "tools": args.tools.name,
               "skill": args.skill.name if args.skill else None, "by_type": summarize(records)}
    (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    print(json.dumps(summary, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
