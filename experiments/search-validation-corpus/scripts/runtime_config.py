#!/usr/bin/env python3
"""Write the disposable validation runtime's Source/worker and actor files.

The Source file is shared by `search_outbox_worker` and the validation host.
`parser_build_id` and each profile's `parser_build_sha256` are the SHA-256 of
the extraction worker executable (the worker refuses anything else). Only the
formats listed in --formats are registered; others stay unsupported.

Usage:
  runtime_config.py --root DIR --worker PATH --source-id UUID
                    [--formats text] [--pdfium-sha256 HEX] [--vector-model DIR]
"""

import argparse
import hashlib
import json
import secrets
from pathlib import Path

BUDGET_KEYS = ["InputBytes", "ZipEntries", "ZipEntryBytes", "ZipTotalBytes", "ZipDepth", "Units",
               "UnitUtf8Bytes", "WorkerOutputBytes", "XmlDepth", "XmlNodes", "PdfPages",
               "PdfOperations", "HtmlNodes", "CsvRecords", "CsvFieldBytes"]


def limits(fmt: str) -> dict:
    values = {key: 0 for key in BUDGET_KEYS}
    values.update({"InputBytes": 8 * 1024 * 1024, "Units": 100_000, "UnitUtf8Bytes": 65_536,
                   "WorkerOutputBytes": 16_777_216})
    if fmt == "Pdf":
        values.update({"PdfPages": 1_024, "PdfOperations": 1_000_000})
    if fmt == "Html":
        values.update({"HtmlNodes": 200_000})
    return values


def definition(fmt: str, worker_sha: list[int], pdfium: list[int] | None) -> dict:
    return {
        "format": fmt,
        "parser_name": "search-extraction-worker",
        "parser_version": "1",
        "parser_build_sha256": worker_sha,
        "native_binary_sha256": pdfium if fmt == "Pdf" else None,
        "scope_revision": 1,
        "segmentation_revision": 1,
        "normalization_revision": 1,
        "locator_revision": 1,
        "format_settings": {"Text": {"charset": "utf-8"}} if fmt == "Text" else "None",
        "limits": limits(fmt),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--worker", required=True, type=Path)
    parser.add_argument("--source-id", required=True)
    parser.add_argument("--formats", default="text")
    parser.add_argument("--pdfium-sha256")
    parser.add_argument("--vector-model", type=Path)
    parser.add_argument("--bind", default="127.0.0.1:8090")
    parser.add_argument("--analyzer", default="tantivy-default-0.26.2",
                        help="tantivy-default-0.26.2 (legacy) or tantivy-0.26.2-cjk-bigram-v1")
    args = parser.parse_args()
    digest = hashlib.sha256(args.worker.read_bytes()).digest()
    worker_sha = list(digest)
    pdfium = list(bytes.fromhex(args.pdfium_sha256)) if args.pdfium_sha256 else None
    formats = [{"text": "Text", "pdf": "Pdf", "html": "Html"}[f] for f in args.formats.split(",")]
    source = {
        "tenant": "validation",
        "source_id": args.source_id,
        "source_name": "validation-documents",
        "document_adapter_ref": "document-platform-v1",
        "deployment_revision": 1,
        "registration_revision": 1,
        "visibility_revision": 1,
        "allowed_resource_kinds": ["Knowledge"],
        "supported_modes": ["LocalDirectory", "LocalContentSearch"],
        "enumeration_semantics": "Complete",
        # The tested Document Version lens (document_discovery::index_config).
        "lens": {
            "lens_id": "document-version",
            "lens_version": 1,
            "resource_type": "Knowledge",
            "domain_scope": None,
            "source_scope": args.source_id,
            "identity_fields": ["document_version_id"],
            "high_signal_facets": ["document_type"],
            "searchable_fields": ["title", "permitted_metadata"],
            "applicability_fields": [],
            "temporal_fields": [],
            "relation_fields": [],
            "extraction_policy": None,
            "projection_policy": None,
        },
        "projection_schema_version": "schema-1",
        "analyzer_version": args.analyzer,
        "semantic_registry_version": "validation-1",
        "lexical_root": str(args.root / "lexical"),
        "file_root": str(args.root / "storage"),
        "extraction_worker": str(args.worker),
        "parser_build_id": "sha256:" + digest.hex(),
        "profiles": [definition(f, worker_sha, pdfium) for f in formats],
        "source_lease_ms": 120_000,  # > 3 x the 30 s delivery renewal interval
        "guard_ttl_ms": 120_000,
        "vector": {"enabled": args.vector_model is not None,
                   "model_dir": str(args.vector_model or "/nonexistent")},
    }
    (args.root / "config").mkdir(parents=True, exist_ok=True)
    (args.root / "config" / "source.json").write_text(json.dumps(source, indent=2))
    actors_path = args.root / "config" / "actors.json"
    if not actors_path.exists():
        # Synthetic actors: tokens are local test values, not credentials.
        actors = {"bind": args.bind, "profile": "exploratory", "actors": [
            {"token": secrets.token_hex(16), "principal": "poc-human", "groups": ["poc-users"], "granted": True},
            {"token": secrets.token_hex(16), "principal": "reader-a", "groups": ["readers-a"], "granted": True},
            {"token": secrets.token_hex(16), "principal": "outsider", "groups": [], "granted": True},
            {"token": secrets.token_hex(16), "principal": "ungranted", "groups": ["poc-users"], "granted": False},
        ]}
        actors_path.write_text(json.dumps(actors, indent=2))
        actors_path.chmod(0o600)
    print(json.dumps({"source": str(args.root / "config" / "source.json"),
                      "actors": str(actors_path), "parser_build_id": source["parser_build_id"]}))


if __name__ == "__main__":
    main()
