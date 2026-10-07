#!/usr/bin/env python3
"""Measure how long one document change takes to become searchable.

Each round creates and publishes one small synthetic document through the
Common Document API, then polls the validation Search API for a token that
only that document contains. Between rounds it records the Search worker's
resident memory and the size of the generation tables, so the growth per
generation can be compared before and after a storage change.

Usage: reflect_probe.py --rounds 5 --out results.jsonl [--timeout 300]
The synthetic texts are disposable and carry no customer data.
"""

import argparse
import json
import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ingest  # noqa: E402
import search_client  # noqa: E402

TABLES = ("search_generation_payload", "search_unit_segment", "search_generation_segment",
          "search_vector_entry")


def worker_rss_kib() -> int | None:
    out = subprocess.run(["ps", "-C", "search_outbox_worker", "-o", "rss="],
                         capture_output=True, text=True).stdout.split()
    return int(out[0]) if out else None


def table_bytes() -> dict:
    url = os.environ["DATABASE_URL"]
    sizes = {}
    for table in TABLES:
        out = subprocess.run(
            ["psql", url, "-Atc",
             f"SELECT COALESCE(pg_total_relation_size(to_regclass('{table}')), 0)"],
            capture_output=True, text=True)
        sizes[table] = int(out.stdout.strip() or 0)
    return sizes


def searchable(token: str, actor: str) -> bool:
    status, body, _ = search_client.search(actor, token, "bodyRequired", 5)
    return status == 200 and bool(body.get("items"))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--base", default="http://127.0.0.1:8080")
    parser.add_argument("--actor", default="poc-human")
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--state", type=Path, default=Path.home() / "kpval/ingest/reflect.folder.json")
    args = parser.parse_args()
    api = ingest.Api(args.base)
    folder_id = ingest.ensure_folder(api, "reflect-probe", args.state)
    with open(args.out, "a") as out:
        for round_ in range(args.rounds):
            token = "反映試験" + uuid.uuid4().hex[:12]
            before = {"rss_kib": worker_rss_kib(), "tables": table_bytes()}
            text = f"{token}\nこの文書は検索反映の計測用の合成文書です。\n".encode()
            body, content_type = ingest.multipart(
                {"folderId": folder_id, "title": token, "documentMetadata": {}, "versionMetadata": {}},
                text, f"{token}.txt")
            started = time.monotonic()
            created = api.call("POST", "/v1/documents", body, content_type)
            detail = api.json("GET", f"/v1/documents/{created['documentId']}?view=authoring")
            api.json("POST", f"/v1/documents/{created['documentId']}/versions/"
                     f"{created['documentVersionId']}:publish",
                     {"operationId": ingest.uuid7(), "expectedRevision": detail["revision"]})
            published = time.monotonic()
            peak = before["rss_kib"] or 0
            found = None
            while time.monotonic() - published < args.timeout:
                peak = max(peak, worker_rss_kib() or 0)
                if searchable(token, args.actor):
                    found = time.monotonic()
                    break
                time.sleep(0.5)
            first_query = None
            if found is not None:
                query_started = time.monotonic()
                searchable(token, args.actor)
                first_query = round((time.monotonic() - query_started) * 1000, 1)
            record = {
                "round": round_, "publish_s": round(published - started, 2),
                "reflect_s": round(found - published, 2) if found else None,
                "next_query_ms": first_query, "rss_before_kib": before["rss_kib"],
                "rss_peak_kib": peak, "tables_before": before["tables"],
                "tables_after": table_bytes(),
            }
            out.write(json.dumps(record) + "\n")
            out.flush()
            print(json.dumps({k: record[k] for k in ("round", "reflect_s", "rss_peak_kib")}), flush=True)
            time.sleep(5)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
