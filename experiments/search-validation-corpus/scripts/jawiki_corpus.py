#!/usr/bin/env python3
"""Build a search-input corpus from one jawiki multistream dump part.

Search input keeps the title, headings and body text, including the visible
text of links. Categories, files, templates, references, tables, link targets
and every URL are removed from it. Provenance (page id, revision, timestamp,
source URL, licence, hashes) and the original link targets go to separate
files that are never given to the indexer. File names and document ids are
HMAC values of the page id under a local secret, so they reveal nothing.

Usage:
  jawiki_corpus.py --dump PART.bz2 --out DIR --limit N [--min-chars 800]
"""

import argparse
import bz2
import hashlib
import hmac
import json
import os
import re
import secrets
import sys
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

import mwparserfromhell

CONVERTER = "jawiki_corpus.py/1"
LICENSE = "CC BY-SA 4.0 / GFDL (Wikipedia contributors)"
DROP_LINK_PREFIXES = (
    "file:", "image:", "ファイル:", "画像:", "media:",
    "category:", "カテゴリ:",
)
DROP_TAGS = {"ref", "references", "gallery", "math", "score", "timeline",
             "syntaxhighlight", "source", "imagemap", "mapframe", "templatestyles"}
DISAMBIGUATION = re.compile(r"\{\{\s*(aimai|曖昧さ回避|Disambig|人名の曖昧さ回避|地名の曖昧さ回避)", re.I)


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def secret(root: Path) -> bytes:
    path = root / "secret-salt"
    if not path.exists():
        path.write_text(secrets.token_hex(32))
        os.chmod(path, 0o600)
    return bytes.fromhex(path.read_text().strip())


def opaque(salt: bytes, kind: str, key: str) -> str:
    return hmac.new(salt, f"{kind}:{key}".encode(), hashlib.sha256).hexdigest()[:24]


def template_text(template) -> str:
    """The reader-visible text of inline templates; '' for everything else."""
    name = str(template.name).strip().lower().replace("_", " ")
    positional = [str(p.value).strip() for p in template.params if not p.showkey]
    if not positional:
        return ""
    if name.startswith("lang") or name in {"ruby", "ルビ", "en", "small", "nowrap", "fontsize"}:
        return positional[-1]
    if name in {"仮リンク", "illm", "interlanguage link"}:
        return positional[0]
    if name in {"読み仮名", "読み仮名 ruby不使用", "読み仮名_ruby不使用"}:
        return positional[0] + (f"（{positional[1]}）" if len(positional) > 1 else "")
    return ""


def clean(wikitext: str) -> tuple[str, list[str]]:
    """Plain search text and the original link targets (kept out of it)."""
    code = mwparserfromhell.parse(wikitext)
    targets = []
    for link in code.filter_wikilinks(recursive=True):
        title = str(link.title).strip()
        lowered = title.lower()
        if lowered.startswith(DROP_LINK_PREFIXES) or title.startswith(":"):
            try:
                code.remove(link)
            except ValueError:
                pass
            continue
        targets.append(title.split("#", 1)[0].strip())
    for template in code.filter_templates(recursive=False):
        visible = template_text(template)
        try:
            if visible:
                code.replace(template, visible)
            else:
                code.remove(template)
        except ValueError:
            pass
    for tag in code.filter_tags(recursive=False):
        name = str(tag.tag).strip().lower()
        if name in DROP_TAGS or name in {"table"}:
            try:
                code.remove(tag)
            except ValueError:
                pass
    for comment in code.filter_comments():
        try:
            code.remove(comment)
        except ValueError:
            pass
    text =code.strip_code(normalize=True, collapse=True, keep_template_params=False)
    # Leftover table/markup fragments and bare URLs never reach search input.
    out = []
    for line in text.splitlines():
        line = re.sub(r"https?://\S+", "", line).strip()
        if not line or line.startswith(("{|", "|}", "|-", "!")):
            continue
        out.append(line)
    return "\n".join(out), targets


def pages(dump: Path):
    with bz2.open(dump, "rb") as stream:
        for _, element in ET.iterparse(stream, events=("end",)):
            if local_name(element.tag) == "page":
                record = {}
                revision_seen = False
                for child in element:
                    child_name = local_name(child.tag)
                    if child_name == "title":
                        record["title"] = child.text or ""
                    elif child_name == "ns":
                        record["ns"] = int(child.text or "0")
                    elif child_name == "id":
                        record["page_id"] = int(child.text)
                    elif child_name == "redirect":
                        record["redirect"] = child.attrib.get("title", "")
                    elif child_name == "revision" and not revision_seen:
                        revision_seen = True
                        for field in child:
                            field_name = local_name(field.tag)
                            if field_name == "id":
                                record["revision_id"] = int(field.text)
                            elif field_name == "timestamp":
                                record["timestamp"] = field.text
                            elif field_name == "text":
                                record["text"] = field.text or ""
                yield record
                element.clear()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--limit", required=True, type=int)
    parser.add_argument("--min-chars", type=int, default=800)
    parser.add_argument("--max-chars", type=int, default=200_000)
    parser.add_argument("--dump-date", default="20261001")
    args = parser.parse_args()

    data_root = args.out.parent.parent
    salt = secret(data_root)
    input_dir = args.out / "input"
    truth_dir = args.out / "truth"
    input_dir.mkdir(parents=True, exist_ok=True)
    truth_dir.mkdir(parents=True, exist_ok=True)
    dump_sha = None
    accepted = 0
    seen = {"pages": 0, "main": 0, "redirect": 0, "disambiguation": 0, "short": 0, "long": 0}
    redirects = {}
    retrieved = datetime.now(timezone.utc).isoformat(timespec="seconds")
    with open(args.out / "documents.jsonl", "w") as documents, \
         open(args.out / "provenance.jsonl", "w") as provenance, \
         open(truth_dir / "links.jsonl", "w") as links:
        for page in pages(args.dump):
            seen["pages"] += 1
            if page.get("ns") != 0:
                continue
            seen["main"] += 1
            if "redirect" in page:
                seen["redirect"] += 1
                redirects[page["title"]] = page["redirect"]
                continue
            raw = page.get("text", "")
            if DISAMBIGUATION.search(raw):
                seen["disambiguation"] += 1
                continue
            body, targets = clean(raw)
            if len(body) < args.min_chars:
                seen["short"] += 1
                continue
            if len(body) > args.max_chars:
                seen["long"] += 1
                continue
            doc = opaque(salt, "jawiki", str(page["page_id"]))
            text = f"{page['title']}\n\n{body}\n"
            data = text.encode("utf-8")
            (input_dir / f"{doc}.txt").write_bytes(data)
            documents.write(json.dumps({"doc": doc, "title": page["title"], "file": f"input/{doc}.txt",
                                        "bytes": len(data)}, ensure_ascii=False) + "\n")
            provenance.write(json.dumps({
                "doc": doc,
                "source": "jawiki",
                "page_id": page["page_id"],
                "revision_id": page.get("revision_id"),
                "revision_timestamp": page.get("timestamp"),
                "title": page["title"],
                "source_url": f"https://ja.wikipedia.org/wiki/{page['title'].replace(' ', '_')}?oldid={page.get('revision_id')}",
                "dump": f"jawiki-{args.dump_date}/{args.dump.name}",
                "retrieved_at": retrieved,
                "license": LICENSE,
                "attribution": "Wikipedia日本語版の記事を変換（リンク先・カテゴリ・テンプレート・表を除去）",
                "converter": CONVERTER,
                "raw_sha256": hashlib.sha256(raw.encode("utf-8")).hexdigest(),
                "input_sha256": hashlib.sha256(data).hexdigest(),
            }, ensure_ascii=False) + "\n")
            links.write(json.dumps({"doc": doc, "title": page["title"], "targets": sorted(set(targets))},
                                   ensure_ascii=False) + "\n")
            accepted += 1
            if accepted >= args.limit:
                break
    (truth_dir / "redirects.json").write_text(json.dumps(redirects, ensure_ascii=False))
    summary = {"accepted": accepted, **seen, "converter": CONVERTER, "dump": str(args.dump.name),
               "min_chars": args.min_chars, "max_chars": args.max_chars}
    (args.out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
