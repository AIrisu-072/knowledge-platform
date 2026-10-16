#!/usr/bin/env python3
"""Build a search-input corpus of laws from the e-Gov 法令API Version 2.

Each listed law becomes one document per revision: the revision in force
now and, when `previous` is set, the one in force immediately before it.
Search input is the law title, number and the article text (articles as
paragraph blocks, items indented). Revision ids, promulgation/enforcement
dates, the amending law, source URLs and hashes go to the provenance file
only, so a question about dates must be answered from the text itself
(e.g. 附則の施行期日) or is scored in the separate structured lane.

Usage: egov_laws.py --laws sources/laws.json --out DIR [--as-of 2026-10-06]
"""

import argparse
import base64
import hashlib
import hmac
import json
import os
import secrets
import sys
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

API = "https://laws.e-gov.go.jp/api/2"
CONVERTER = "egov_laws.py/1"
LICENSE = "e-Gov法令検索 法令データ（二次利用に制限なし。出典：e-Gov法令検索）"
HEADINGS = {"PartTitle", "ChapterTitle", "SectionTitle", "SubsectionTitle", "DivisionTitle"}
SKIP = {"TOC", "Rt"}


def secret(root: Path) -> bytes:
    path = root / "secret-salt"
    if not path.exists():
        path.write_text(secrets.token_hex(32))
        os.chmod(path, 0o600)
    return bytes.fromhex(path.read_text().strip())


def opaque(salt: bytes, key: str) -> str:
    return hmac.new(salt, f"egov:{key}".encode(), hashlib.sha256).hexdigest()[:24]


def get_json(path: str, cache: Path) -> dict:
    if cache.exists():
        return json.loads(cache.read_text())
    request = urllib.request.Request(f"{API}/{path}", headers={"User-Agent": "kp-search-validation/1"})
    with urllib.request.urlopen(request, timeout=120) as response:
        body = response.read()
    cache.write_bytes(body)
    time.sleep(1.0)  # be gentle with the public API
    return json.loads(body)


def flat(element) -> str:
    return "".join(text for text in element.itertext() if text).replace("\n", "").strip()


def child(element, tag):
    return next((c for c in element if c.tag == tag), None)


def item_lines(element, depth: int) -> list[str]:
    """An Item / SubitemN with its title, sentence and nested subitems."""
    lines = []
    title = next((c for c in element if c.tag.endswith("Title")), None)
    sentence = next((c for c in element if c.tag.endswith("Sentence")), None)
    parts = [flat(title) if title is not None else "", flat(sentence) if sentence is not None else ""]
    lines.append("　" * depth + "　".join(p for p in parts if p))
    for nested in element:
        if nested.tag.startswith("Subitem") and not nested.tag.endswith(("Title", "Sentence")):
            lines.extend(item_lines(nested, depth + 1))
        elif nested.tag in ("TableStruct", "List"):
            lines.append("　" * (depth + 1) + flat(nested))
    return lines


def paragraph_lines(paragraph, lead: str = "") -> list[str]:
    number = child(paragraph, "ParagraphNum")
    sentence = child(paragraph, "ParagraphSentence")
    head = (flat(number) if number is not None else "") or lead
    text = flat(sentence) if sentence is not None else ""
    lines = [f"{head}　{text}".strip("　")]
    for nested in paragraph:
        if nested.tag == "Item":
            lines.extend(item_lines(nested, 1))
        elif nested.tag in ("TableStruct", "List", "AmendProvision", "StyleStruct", "FormatStruct"):
            lines.append("　" + flat(nested))
    return lines


def article_block(article) -> str:
    caption = child(article, "ArticleCaption")
    title = child(article, "ArticleTitle")
    lines = [flat(caption)] if caption is not None else []
    first = True
    for paragraph in (c for c in article if c.tag == "Paragraph"):
        lead = flat(title) if first and title is not None else ""
        lines.extend(paragraph_lines(paragraph, lead))
        first = False
    return "\n".join(line for line in lines if line)


def render(element, blocks: list[str]) -> None:
    tag = element.tag
    if tag in SKIP:
        return
    if tag in HEADINGS:
        blocks.append(flat(element))
        return
    if tag == "Article":
        blocks.append(article_block(element))
        return
    if tag == "Paragraph":
        blocks.append("\n".join(paragraph_lines(element)))
        return
    if tag == "SupplProvision":
        label = child(element, "SupplProvisionLabel")
        amend = element.attrib.get("AmendLawNum", "")
        blocks.append(((flat(label) if label is not None else "附則") + (f"（{amend}）" if amend else "")))
        for c in element:
            if c.tag != "SupplProvisionLabel":
                render(c, blocks)
        return
    if tag in ("AppdxTable", "AppdxNote", "AppdxStyle", "Appdx", "AppdxFormat", "AppdxFig"):
        text = flat(element)
        if text:
            blocks.append(text)
        return
    if tag in ("LawTitle", "LawNum", "EnactStatement", "Preamble"):
        blocks.append(flat(element))
        return
    for c in element:
        render(c, blocks)


def law_text(xml_bytes: bytes) -> str:
    root = ET.fromstring(xml_bytes)
    blocks: list[str] = []
    law_num = child(root, "LawNum")
    body = child(root, "LawBody")
    title = child(body, "LawTitle")
    blocks.append(flat(title))
    if law_num is not None:
        blocks.append(flat(law_num))
    for c in body:
        if c.tag != "LawTitle":
            render(c, blocks)
    return "\n\n".join(b for b in blocks if b.strip()) + "\n"


def pick_revisions(revisions: list[dict], previous: bool) -> list[tuple[str, dict]]:
    current = next((r for r in revisions if r.get("current_revision_status") == "CurrentEnforced"), None)
    picked = [("current", current)] if current else []
    if previous and current:
        older = [r for r in revisions
                 if r.get("current_revision_status") == "PreviousEnforced"
                 and (r.get("amendment_enforcement_date") or "") < (current.get("amendment_enforcement_date") or "")]
        older.sort(key=lambda r: r.get("amendment_enforcement_date") or "", reverse=True)
        if older:
            picked.append(("previous", older[0]))
    return picked


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--laws", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--raw", type=Path, default=Path.home() / "kp-validation-data/raw/egov")
    args = parser.parse_args()
    salt = secret(args.out.parent.parent)
    (args.out / "input").mkdir(parents=True, exist_ok=True)
    args.raw.mkdir(parents=True, exist_ok=True)
    retrieved = datetime.now(timezone.utc).isoformat(timespec="seconds")
    listing = json.loads(args.laws.read_text())["laws"]
    summary = {"laws": 0, "documents": 0, "missing": []}
    with open(args.out / "documents.jsonl", "w") as documents, \
         open(args.out / "provenance.jsonl", "w") as provenance:
        for entry in listing:
            title = entry["title"]
            found = get_json(f"laws?law_title={urllib.parse.quote(title)}&limit=50",
                             args.raw / f"search-{hashlib.sha256(title.encode()).hexdigest()[:16]}.json")
            match = next((law for law in found.get("laws", [])
                          if law["revision_info"]["law_title"] == title), None)
            if match is None:
                summary["missing"].append(title)
                continue
            law_id = match["law_info"]["law_id"]
            revisions = get_json(f"law_revisions/{law_id}", args.raw / f"revisions-{law_id}.json")["revisions"]
            summary["laws"] += 1
            for role, revision in pick_revisions(revisions, entry.get("previous", False)):
                revision_id = revision["law_revision_id"]
                data = get_json(f"law_data/{revision_id}?law_full_text_format=xml&response_format=json",
                                args.raw / f"law-{revision_id}.json")
                xml_bytes = base64.b64decode(data["law_full_text"])
                text = law_text(xml_bytes).encode("utf-8")
                doc = opaque(salt, revision_id)
                (args.out / "input" / f"{doc}.txt").write_bytes(text)
                documents.write(json.dumps({"doc": doc, "title": title, "file": f"input/{doc}.txt",
                                            "bytes": len(text)}, ensure_ascii=False) + "\n")
                provenance.write(json.dumps({
                    "doc": doc,
                    "source": "egov",
                    "role": role,
                    "law_id": law_id,
                    "law_num": match["law_info"].get("law_num"),
                    "law_revision_id": revision_id,
                    "title": title,
                    "promulgation_date": match["law_info"].get("promulgation_date"),
                    "amendment_law_title": revision.get("amendment_law_title"),
                    "amendment_promulgate_date": revision.get("amendment_promulgate_date"),
                    "amendment_enforcement_date": revision.get("amendment_enforcement_date"),
                    "revision_status": revision.get("current_revision_status"),
                    "source_url": f"https://laws.e-gov.go.jp/law/{law_id}",
                    "api_url": f"{API}/law_data/{revision_id}",
                    "retrieved_at": retrieved,
                    "license": LICENSE,
                    "attribution": "出典：e-Gov法令検索（https://laws.e-gov.go.jp/）の法令データを本文テキストへ変換",
                    "converter": CONVERTER,
                    "raw_sha256": hashlib.sha256(xml_bytes).hexdigest(),
                    "input_sha256": hashlib.sha256(text).hexdigest(),
                }, ensure_ascii=False) + "\n")
                summary["documents"] += 1
    (args.out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
