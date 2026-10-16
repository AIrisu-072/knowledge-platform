#!/bin/sh
# Writes the Search Source file shared by search_outbox_worker and
# search-server, from this image's extraction worker. The worker refuses a
# parser build other than the executable's SHA-256, so the file is derived
# here rather than kept in the repository.
#
# Usage: search-source.sh OUTPUT
# Environment (defaults in parentheses):
#   KP_SEARCH_SOURCE_ID    UUID of the Document Source (required)
#   KP_SEARCH_TENANT       tenant (poc)
#   KP_SEARCH_ANALYZER     lexical analyzer (tantivy-0.26.2-cjk-bigram-v1)
#   KP_SEARCH_LEXICAL_ROOT lexical index directory (/var/lib/knowledge-platform/search/lexical)
#   KP_STORAGE_ROOT        Document original storage (/var/lib/knowledge-platform/storage)
#   KP_SEARCH_VECTOR_MODEL_DIR  pinned embedding model directory (unset, or no
#                          model.safetensors in it: Vector disabled)
set -eu

output=${1:?usage: search-source.sh OUTPUT}
source_id=${KP_SEARCH_SOURCE_ID:?KP_SEARCH_SOURCE_ID is required}
tenant=${KP_SEARCH_TENANT:-poc}
analyzer=${KP_SEARCH_ANALYZER:-tantivy-0.26.2-cjk-bigram-v1}
lexical_root=${KP_SEARCH_LEXICAL_ROOT:-/var/lib/knowledge-platform/search/lexical}
file_root=${KP_STORAGE_ROOT:-/var/lib/knowledge-platform/storage}
worker=/usr/local/bin/search-extraction-worker
model_dir=${KP_SEARCH_VECTOR_MODEL_DIR:-}

case "$source_id" in
  *[!0-9a-f-]*|"") echo "search-source.sh: KP_SEARCH_SOURCE_ID must be a lowercase UUID" >&2; exit 64 ;;
esac

digest=$(sha256sum "$worker" | cut -d' ' -f1)
# The digest as a JSON array of byte values.
bytes=$(printf '%s' "$digest" | sed 's/../& /g' | {
  first=1
  for pair in $(cat); do
    if [ "$first" = 1 ]; then first=0; else printf ','; fi
    printf '%d' "0x$pair"
  done
})

if [ -n "$model_dir" ] && [ -f "$model_dir/model.safetensors" ]; then
  vector_enabled=true
else
  vector_enabled=false
  model_dir=/nonexistent
fi

mkdir -p "$(dirname "$output")" "$lexical_root"
tmp="$output.tmp.$$"
cat > "$tmp" <<EOF
{
  "tenant": "$tenant",
  "source_id": "$source_id",
  "source_name": "documents",
  "document_adapter_ref": "document-platform-v1",
  "deployment_revision": 1,
  "registration_revision": 1,
  "visibility_revision": 1,
  "allowed_resource_kinds": ["Knowledge"],
  "supported_modes": ["LocalDirectory", "LocalContentSearch"],
  "enumeration_semantics": "Complete",
  "lens": {
    "lens_id": "document-version",
    "lens_version": 1,
    "resource_type": "Knowledge",
    "domain_scope": null,
    "source_scope": "$source_id",
    "identity_fields": ["document_version_id"],
    "high_signal_facets": ["document_type"],
    "searchable_fields": ["title", "permitted_metadata"],
    "applicability_fields": [],
    "temporal_fields": [],
    "relation_fields": [],
    "extraction_policy": null,
    "projection_policy": null
  },
  "projection_schema_version": "schema-1",
  "analyzer_version": "$analyzer",
  "semantic_registry_version": "poc-1",
  "lexical_root": "$lexical_root",
  "file_root": "$file_root",
  "extraction_worker": "$worker",
  "parser_build_id": "sha256:$digest",
  "profiles": [
    {
      "format": "Text",
      "parser_name": "search-extraction-worker",
      "parser_version": "1",
      "parser_build_sha256": [$bytes],
      "native_binary_sha256": null,
      "scope_revision": 1,
      "segmentation_revision": 1,
      "normalization_revision": 1,
      "locator_revision": 1,
      "format_settings": {"Text": {"charset": "utf-8"}},
      "limits": {
        "InputBytes": 8388608, "ZipEntries": 0, "ZipEntryBytes": 0, "ZipTotalBytes": 0,
        "ZipDepth": 0, "Units": 100000, "UnitUtf8Bytes": 65536, "WorkerOutputBytes": 16777216,
        "XmlDepth": 0, "XmlNodes": 0, "PdfPages": 0, "PdfOperations": 0, "HtmlNodes": 0,
        "CsvRecords": 0, "CsvFieldBytes": 0
      }
    }
  ],
  "source_lease_ms": 120000,
  "guard_ttl_ms": 120000,
  "vector": {"enabled": $vector_enabled, "model_dir": "$model_dir"}
}
EOF
mv "$tmp" "$output"
echo "search-source.sh: wrote $output (parser sha256:$digest)"
