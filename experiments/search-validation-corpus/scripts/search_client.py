#!/usr/bin/env python3
"""Minimal client of the validation Search API host.

  search_client.py search "query" [--coverage bodyRequired] [--actor poc-human] [--size 10]
  search_client.py discover "query" [--actor poc-human]

The actor's synthetic token is read from the local actors file; it is never
printed. Prints the JSON response, or the HTTP status with the problem body.
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

ACTORS = Path(os.environ.get("SEARCH_VALIDATION_ACTORS", Path.home() / "kpval/config/actors.json"))


def token_for(principal: str) -> tuple[str, str]:
    actors = json.loads(ACTORS.read_text())
    bind = actors["bind"]
    for actor in actors["actors"]:
        if actor["principal"] == principal:
            return actor["token"], f"http://{bind}"
    raise SystemExit(f"unknown actor {principal}")


def post(principal: str, path: str, payload: dict) -> tuple[int, dict, float]:
    token, base = token_for(principal)
    request = urllib.request.Request(f"{base}{path}", data=json.dumps(payload).encode(), method="POST")
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Content-Type", "application/json")
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            body = json.loads(response.read() or b"null")
            return response.status, body, (time.perf_counter() - started) * 1000
    except urllib.error.HTTPError as error:
        raw = error.read()
        try:
            body = json.loads(raw)
        except ValueError:
            body = {"raw": raw[:300].decode("utf-8", "replace")}
        return error.code, body, (time.perf_counter() - started) * 1000


def get(principal: str, path: str) -> tuple[int, dict, float]:
    token, base = token_for(principal)
    request = urllib.request.Request(f"{base}{path}", method="GET")
    request.add_header("Authorization", f"Bearer {token}")
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, json.loads(response.read() or b"null"), (time.perf_counter() - started) * 1000
    except urllib.error.HTTPError as error:
        return error.code, {"raw": error.read()[:300].decode("utf-8", "replace")}, (time.perf_counter() - started) * 1000


def search(principal: str, query: str, coverage: str = "bodyRequired", size: int = 10, cursor: str | None = None):
    payload = {"query": query, "coverage": coverage, "pageSize": size}
    if cursor:
        payload["cursor"] = cursor
    return post(principal, "/v1/search", payload)


def discover(principal: str, query: str, coverage: str = "bodyRequired", purpose: str = "find the documents"):
    return post(principal, "/v1/discover", {
        "need": {"purpose": purpose, "requiredResourceTypes": ["knowledge"],
                 "requiredClaimIds": [str(uuid.uuid4())]},
        "query": query, "coverage": coverage})


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=["search", "discover", "sources"])
    parser.add_argument("query", nargs="?")
    parser.add_argument("--actor", default="poc-human")
    parser.add_argument("--coverage", default="bodyRequired")
    parser.add_argument("--size", type=int, default=10)
    args = parser.parse_args()
    if args.kind == "search":
        status, body, ms = search(args.actor, args.query, args.coverage, args.size)
    elif args.kind == "discover":
        status, body, ms = discover(args.actor, args.query, args.coverage)
    else:
        status, body, ms = get(args.actor, "/v1/sources?pageSize=100")
    print(json.dumps({"status": status, "ms": round(ms, 1), "body": body}, ensure_ascii=False, indent=1))
    return 0 if status == 200 else 1


if __name__ == "__main__":
    sys.exit(main())
