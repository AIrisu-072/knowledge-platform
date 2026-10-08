#!/usr/bin/env python3
"""Measure how fast a burst of document changes is drained.

Creates and publishes N small synthetic documents back to back through the
Common Document API, then polls the outbox until no delivery is pending and
the last document is searchable. Prints the elapsed times and event counts.

Usage: bulk_probe.py --count 30
"""

import argparse
import os
import subprocess
import time
import uuid
from pathlib import Path

import ingest
import reflect_probe

PENDING = ("SELECT count(*) FROM outbox_delivery_state WHERE processing_state = 'PENDING'",)


def pending() -> int:
    out = subprocess.run(["psql", os.environ["DATABASE_URL"], "-Atc", PENDING[0]],
                         capture_output=True, text=True)
    return int(out.stdout.strip() or 0)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--count", type=int, default=30)
    parser.add_argument("--base", default="http://127.0.0.1:8080")
    parser.add_argument("--actor", default="poc-human")
    parser.add_argument("--timeout", type=float, default=3600)
    parser.add_argument("--state", type=Path, default=Path.home() / "kpval/ingest/reflect.folder.json")
    args = parser.parse_args()
    api = ingest.Api(args.base)
    folder_id = ingest.ensure_folder(api, "reflect-probe", args.state)
    started = time.monotonic()
    last = None
    for _ in range(args.count):
        last = "一括試験" + uuid.uuid4().hex[:12]
        body, content_type = ingest.multipart(
            {"folderId": folder_id, "title": last, "documentMetadata": {}, "versionMetadata": {}},
            f"{last}\n一括反映の計測用の合成文書です。\n".encode(), f"{last}.txt")
        created = api.call("POST", "/v1/documents", body, content_type)
        detail = api.json("GET", f"/v1/documents/{created['documentId']}?view=authoring")
        api.json("POST", f"/v1/documents/{created['documentId']}/versions/"
                 f"{created['documentVersionId']}:publish",
                 {"operationId": ingest.uuid7(), "expectedRevision": detail["revision"]})
    submitted = time.monotonic()
    peak = pending()
    while time.monotonic() - started < args.timeout:
        now = pending()
        peak = max(peak, now)
        if now == 0 and reflect_probe.searchable(last, args.actor):
            break
        time.sleep(1)
    done = time.monotonic()
    print(f"documents={args.count} submit_s={submitted - started:.1f} drained_s={done - started:.1f} "
          f"peak_pending={peak}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
