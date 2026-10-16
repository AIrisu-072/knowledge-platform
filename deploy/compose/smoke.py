#!/usr/bin/env python3
"""Acceptance check of the compose stack (`mise run stack:smoke`), standard library only.

1. Registers and publishes two text documents through the human Document
   server: one in a folder every PoC profile may read, one in a folder whose
   explicit policy grants only the human group.
2. Waits until the Search worker has published them, then checks through the
   two Search servers that the human profile finds both and the agent profile
   finds only the shared one (Search and Discover).
3. Checks that a call without a token, or with a wrong one, gets 401.

Tokens are read from the stack's secret files and never printed.
Usage: smoke.py [--stack-dir DIR] [--timeout SECONDS] [--check-only TITLES_FILE]
"""

import argparse
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

DOCUMENT = os.environ.get("KP_SMOKE_DOCUMENT_URL", "http://127.0.0.1:8080")
SEARCH = {
    "human": os.environ.get("KP_SMOKE_SEARCH_HUMAN_URL", "http://127.0.0.1:8090"),
    "agent": os.environ.get("KP_SMOKE_SEARCH_AGENT_URL", "http://127.0.0.1:8091"),
}


def uuid7() -> str:
    millis = int(time.time() * 1000)
    raw = bytearray(millis.to_bytes(6, "big") + secrets.token_bytes(10))
    raw[6] = (raw[6] & 0x0F) | 0x70
    raw[8] = (raw[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(raw)))


def call(base, method, path, body=None, content_type=None, token=None):
    request = urllib.request.Request(f"{base}{path}", data=body, method=method)
    request.add_header("Accept", "application/json")
    if content_type:
        request.add_header("Content-Type", content_type)
    if token is not None:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, json.loads(response.read() or b"null")
    except urllib.error.HTTPError as error:
        return error.code, None


def document_json(method, path, payload=None):
    body = json.dumps(payload).encode() if payload is not None else None
    status, data = call(DOCUMENT, method, path, body, "application/json" if body else None)
    if status >= 300:
        raise SystemExit(f"smoke: Document {method} {path} -> {status}")
    return data


def create_folder(name, human_only):
    root = document_json("GET", "/v1/folders/root")
    folder_id = str(uuid.uuid4())
    document_json("POST", "/v1/folders", {
        "operationId": uuid7(), "folderId": folder_id, "parentFolderId": root["folderId"],
        "expectedParentRevision": root["revision"], "name": name, "reason": "stack smoke",
    })
    if human_only:
        policy = document_json("GET", f"/v1/folders/{folder_id}/access-policy")
        document_json("PUT", f"/v1/folders/{folder_id}/access-policy", {
            "operationId": uuid7(), "expectedPolicyRevision": policy["policyRevision"],
            "reason": "stack smoke: human only", "mode": "explicit",
            "grants": [{"subjectKind": "group", "identityProvider": "poc", "subjectId": "poc-users",
                        "actions": ["read", "readHistory", "write", "publish", "administer"]}],
        })
    return folder_id


def publish_text(folder_id, title, text):
    boundary = "----kpsmoke" + secrets.token_hex(12)
    request = {"folderId": folder_id, "title": title, "documentMetadata": {}, "versionMetadata": {}}
    body = b"".join([
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\n"
        f"Content-Type: application/json\r\n\r\n".encode() + json.dumps(request).encode() + b"\r\n",
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"smoke.txt\"\r\n"
        f"Content-Type: text/plain\r\n\r\n".encode() + text.encode() + b"\r\n",
        f"--{boundary}--\r\n".encode(),
    ])
    status, created = call(DOCUMENT, "POST", "/v1/documents", body,
                           f"multipart/form-data; boundary={boundary}")
    if status >= 300:
        raise SystemExit(f"smoke: Document registration -> {status}")
    document_id, version_id = created["documentId"], created["documentVersionId"]
    detail = document_json("GET", f"/v1/documents/{document_id}?view=authoring")
    document_json("POST", f"/v1/documents/{document_id}/versions/{version_id}:publish",
                  {"operationId": uuid7(), "expectedRevision": detail["revision"]})


def titles(profile, token, query):
    status, page = call(SEARCH[profile], "POST", "/v1/search",
                        json.dumps({"query": query, "coverage": "bodyRequired", "pageSize": 10}).encode(),
                        "application/json", token)
    if status != 200:
        return status, set()
    return status, {item.get("title") for item in page.get("items", [])}


def discovered(profile, token, query):
    status, result = call(SEARCH[profile], "POST", "/v1/discover",
                          json.dumps({"purpose": "find the documents", "query": query,
                                      "coverage": "bodyRequired"}).encode(),
                          "application/json", token)
    return status, json.dumps(result or {}, ensure_ascii=False)


def wait_for(profile, token, query, title, timeout):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        status, found = titles(profile, token, query)
        last = status
        if title in found:
            return True
        time.sleep(3)
    raise SystemExit(f"smoke: {title!r} not found by {profile} within {timeout} s (last status {last})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stack-dir", type=Path,
                        default=Path(os.environ.get("KP_STACK_DIR", Path.home() / ".knowledge-platform/stack")))
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--state", type=Path, help="write (or, with --check-only, read) the smoke titles")
    parser.add_argument("--check-only", action="store_true", help="re-check titles from --state (after a restart)")
    args = parser.parse_args()
    tokens = {profile: (args.stack_dir / "secrets" / f"search_{profile}_token").read_text().strip()
              for profile in SEARCH}

    if args.check_only:
        state = json.loads(args.state.read_text())
    else:
        run = secrets.token_hex(4)
        state = {"shared_word": f"smokeshared{run}", "private_word": f"smokeprivate{run}"}
        state["shared_title"] = f"stack smoke shared {state['shared_word']}"
        state["private_title"] = f"stack smoke private {state['private_word']}"
        shared = create_folder(f"stack-smoke-{run}-shared", human_only=False)
        private = create_folder(f"stack-smoke-{run}-private", human_only=True)
        publish_text(shared, state["shared_title"], f"共有の文書です。{state['shared_word']}\n")
        publish_text(private, state["private_title"], f"人だけが読める文書です。{state['private_word']}\n")
        if args.state:
            args.state.write_text(json.dumps(state))

    wait_for("human", tokens["human"], state["shared_word"], state["shared_title"], args.timeout)
    wait_for("human", tokens["human"], state["private_word"], state["private_title"], args.timeout)
    wait_for("agent", tokens["agent"], state["shared_word"], state["shared_title"], args.timeout)
    status, found = titles("agent", tokens["agent"], state["private_word"])
    if status != 200 or found:
        raise SystemExit(f"smoke: agent search for the human-only document returned {status} {len(found)} items")
    status, body = discovered("agent", tokens["agent"], state["private_word"])
    if status != 200 or state["private_title"] in body or state["private_word"] in body:
        raise SystemExit(f"smoke: agent discover leaked or failed ({status})")
    status, body = discovered("human", tokens["human"], state["shared_word"])
    if status != 200:
        raise SystemExit(f"smoke: human discover returned {status}")
    for token in (None, "x" * 40):
        status, _ = call(SEARCH["human"], "POST", "/v1/search",
                         json.dumps({"query": state["shared_word"], "coverage": "bodyRequired",
                                     "pageSize": 1}).encode(), "application/json", token)
        if status != 401:
            raise SystemExit(f"smoke: search without a valid token returned {status}, not 401")
    print("smoke: ok (search and discover, document ACL, 401 without a valid token)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
