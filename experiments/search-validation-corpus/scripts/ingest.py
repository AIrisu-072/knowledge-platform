#!/usr/bin/env python3
"""Ingest a converted corpus through the Common Document API and publish it.

For each document of `documents.jsonl`: create it with its text file in the
validation folder (multipart `request` + `file`), read its revision, then
publish the initial version. The resulting ids are appended to the ingest
manifest; a re-run resumes after the last recorded document. An unknown
create outcome (no response) stops the run instead of creating twice, as the
API generates ids server-side.

Usage: ingest.py --corpus DIR --manifest FILE [--base URL] [--limit N]
                 [--folder-name NAME] [--workers 4]
"""

import argparse
import json
import os
import secrets
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def uuid7() -> str:
    millis = int(time.time() * 1000)
    raw = bytearray(millis.to_bytes(6, "big") + secrets.token_bytes(10))
    raw[6] = (raw[6] & 0x0F) | 0x70
    raw[8] = (raw[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(raw)))


class Api:
    def __init__(self, base: str):
        self.base = base.rstrip("/")

    def call(self, method: str, path: str, body: bytes | None = None, content_type: str | None = None):
        request = urllib.request.Request(f"{self.base}{path}", data=body, method=method)
        if content_type:
            request.add_header("Content-Type", content_type)
        request.add_header("Accept", "application/json")
        with urllib.request.urlopen(request, timeout=120) as response:
            return json.loads(response.read() or b"null")

    def json(self, method: str, path: str, payload: dict | None = None):
        body = json.dumps(payload).encode() if payload is not None else None
        return self.call(method, path, body, "application/json" if body is not None else None)


def multipart(request: dict, data: bytes, filename: str) -> tuple[bytes, str]:
    boundary = "----kpval" + secrets.token_hex(12)
    parts = [
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\n"
        f"Content-Type: application/json\r\n\r\n".encode() + json.dumps(request).encode() + b"\r\n",
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n"
        f"Content-Type: text/plain\r\n\r\n".encode() + data + b"\r\n",
        f"--{boundary}--\r\n".encode(),
    ]
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def ensure_folder(api: Api, name: str, state_path: Path) -> str:
    if state_path.exists():
        return json.loads(state_path.read_text())["folderId"]
    root = api.json("GET", "/v1/folders/root")
    folder_id = str(uuid.uuid4())
    api.json("POST", "/v1/folders", {
        "operationId": uuid7(), "folderId": folder_id, "parentFolderId": root["folderId"],
        "expectedParentRevision": root["revision"], "name": name,
        "reason": "Search validation corpus (synthetic, disposable)",
    })
    state_path.write_text(json.dumps({"folderId": folder_id, "name": name}))
    return folder_id


class UnknownOutcome(Exception):
    pass


def ingest_one(api: Api, corpus: Path, folder_id: str, entry: dict) -> dict:
    data = (corpus / entry["file"]).read_bytes()
    body, content_type = multipart(
        {"folderId": folder_id, "title": entry["title"], "documentMetadata": {}, "versionMetadata": {}},
        data, f"{entry['doc']}.txt")
    started = time.monotonic()
    try:
        created = api.call("POST", "/v1/documents", body, content_type)
    except urllib.error.HTTPError as error:
        return {"doc": entry["doc"], "status": "create_failed", "http": error.code,
                "problem": error.read()[:300].decode("utf-8", "replace")}
    except (urllib.error.URLError, TimeoutError) as error:
        raise UnknownOutcome(f"{entry['doc']}: {error}") from error
    created_at = time.monotonic()
    document_id = created["documentId"]
    version_id = created["documentVersionId"]
    detail = api.json("GET", f"/v1/documents/{document_id}?view=authoring")
    try:
        api.json("POST", f"/v1/documents/{document_id}/versions/{version_id}:publish",
                 {"operationId": uuid7(), "expectedRevision": detail["revision"]})
        status = "published"
    except urllib.error.HTTPError as error:
        status = f"publish_failed_{error.code}"
    return {"doc": entry["doc"], "documentId": document_id, "versionId": version_id,
            "fileId": created["fileId"], "status": status, "bytes": len(data),
            "create_ms": round((created_at - started) * 1000, 1),
            "total_ms": round((time.monotonic() - started) * 1000, 1),
            "published_at": time.time()}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--base", default="http://127.0.0.1:8080")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--folder-name", default="validation")
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()
    api = Api(args.base)
    folder_id = ensure_folder(api, args.folder_name, args.manifest.with_suffix(".folder.json"))
    done = set()
    if args.manifest.exists():
        for line in args.manifest.read_text().splitlines():
            done.add(json.loads(line)["doc"])
    entries = [json.loads(line) for line in (args.corpus / "documents.jsonl").read_text().splitlines()]
    entries = [entry for entry in entries if entry["doc"] not in done]
    if args.limit:
        entries = entries[: max(0, args.limit - len(done))]
    lock = threading.Lock()
    counts = {"published": 0, "failed": 0}
    started = time.monotonic()
    with open(args.manifest, "a") as manifest, ThreadPoolExecutor(args.workers) as pool:
        try:
            for record in pool.map(lambda entry: ingest_one(api, args.corpus, folder_id, entry), entries):
                with lock:
                    manifest.write(json.dumps(record, ensure_ascii=False) + "\n")
                    manifest.flush()
                    counts["published" if record["status"] == "published" else "failed"] += 1
        except UnknownOutcome as error:
            print(f"stopped on an unknown create outcome: {error}", file=sys.stderr)
            return 2
    elapsed = time.monotonic() - started
    print(json.dumps({**counts, "seconds": round(elapsed, 1),
                      "docs_per_second": round((counts["published"] + counts["failed"]) / elapsed, 2) if elapsed else None}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
