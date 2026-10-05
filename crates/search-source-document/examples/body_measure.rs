//! P1-V01 capacity probe: one synthetic case through the production host
//! pipeline (raw binding, readers, locator round trip, Unit manifest, Tantivy
//! seal, publication) and BodyOnly queries. Prints one JSON object.
//!
//! Readers run in-process here; the fresh-process Linux sandbox is measured by
//! the runner isolation gates, not by this probe. Driven by `experiments/
//! search-extraction-poc/measure.py`, which adds per-process CPU and peak RSS.

#[path = "../tests/support/body_index.rs"]
mod body_index;
#[path = "../tests/support/body.rs"]
mod body_support;

use std::collections::BTreeMap as Map;
use std::time::Instant;

use body_index::*;
use search_application::ports::{LexicalQuery, LexicalRetrieverPort};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{DiscoveryEvaluationId, NeedId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::knowledge_unit::{BudgetKey, FormatId};
use search_core::temporal::TemporalEvaluationContext;
use search_extraction_core::budget::absolute_ceiling;
use search_source_document::{BodyProfileRegistry, PublishedBody};

const MIB: usize = 1024 * 1024;

/// Every applicable key at the code-enforced absolute ceiling (P1 design
/// ceilings: input 256 MiB, result 16 MiB, 100000 Units, ZIP 20000/64/512 MiB).
fn ceilings(format: FormatId) -> Map<BudgetKey, u64> {
    let mut limits = body_support::limits(format);
    for key in BudgetKey::ALL {
        if limits[&key] > 0 {
            limits.insert(key, absolute_ceiling(key));
        }
    }
    limits
}

fn production_registry() -> BodyProfileRegistry {
    BodyProfileRegistry::new(
        "search-extraction-worker-measure",
        [
            FormatId::Docx,
            FormatId::Text,
            FormatId::Csv,
            FormatId::Html,
            FormatId::Zip,
        ]
        .into_iter()
        .map(|format| {
            let mut definition = body_support::definition(format);
            definition.limits = ceilings(format);
            definition
        })
        .collect(),
    )
    .unwrap()
}

/// Space-separated tokens so every queried literal is also an analyzer token.
fn line(n: usize) -> String {
    format!("行{n} 東京{} 本文{} 共通\n", n % 997, n % 13)
}

fn text_of(bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + 64);
    let mut n = 0;
    while out.len() < bytes {
        out.push_str(&line(n));
        n += 1;
    }
    out
}

fn csv_of(bytes: usize) -> String {
    let mut out = String::from("番号,項目,値\n");
    let mut n = 0;
    while out.len() < bytes {
        out.push_str(&format!("{n},東京{},本文{}\n", n % 997, n % 13));
        n += 1;
    }
    out
}

fn html_of(bytes: usize) -> String {
    let mut out = String::from("<!doctype html><html><body>");
    let mut n = 0;
    while out.len() < bytes {
        out.push_str(&format!("<p>{}</p>", line(n).trim_end()));
        n += 1;
    }
    out.push_str("</body></html>");
    out
}

fn docx_of(bytes: usize) -> Vec<u8> {
    let mut paragraphs = String::new();
    let mut n = 0;
    while paragraphs.len() < bytes {
        paragraphs.push_str(&format!(
            "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
            line(n).trim_end()
        ));
        n += 1;
    }
    let types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{paragraphs}</w:body></w:document>"#
    );
    body_support::zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ])
}

/// One Version per entry; each entry lists its Parts as (bytes, media type).
fn corpus(case: &str, mib: usize) -> Vec<Vec<(Vec<u8>, &'static str)>> {
    let bytes = mib * MIB;
    match case {
        "text" => vec![vec![(text_of(bytes).into_bytes(), "text/plain")]],
        "csv" => vec![vec![(csv_of(bytes).into_bytes(), "text/csv")]],
        "html" => vec![vec![(html_of(bytes).into_bytes(), "text/html")]],
        "docx" => vec![vec![(
            docx_of(bytes),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )]],
        // A small archive that expands to `mib` of highly repetitive text.
        "zip-high-ratio" => vec![vec![(
            body_support::zip(&[("member.txt", line(1).repeat(bytes / 40).as_bytes())]),
            "application/zip",
        )]],
        // One Version with many small Parts.
        "many-parts" => vec![
            (0..200)
                .map(|part| (text_of(5 * 1024 + part).into_bytes(), "text/plain"))
                .collect(),
        ],
        // Many Versions sharing tokens: unique-parent fill under one limit.
        "many-documents" => (0..200)
            .map(|_| vec![(text_of(5 * 1024).into_bytes(), "text/plain")])
            .collect(),
        // The largest single Unit the profile admits.
        "max-unit" => vec![vec![(
            format!("{}\n", "長".repeat(65_536 / 3 - 1)).into_bytes(),
            "text/plain",
        )]],
        other => panic!("unknown case {other}"),
    }
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::now_utc();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::now_v7()),
            intent_signature: IntentSignature::new(IntentFact::new(
                "measure".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![],
            required_claims: vec![],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::now_v7()),
            now,
            now,
            "UTC",
        ),
        access_context: "measure".into(),
    }
}

fn percentile(sorted: &[u128], pct: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

struct Built {
    runtime: MemoryDocumentIndexRuntime,
    key: search_core::projection::ProjectionGenerationKey,
    millis: u128,
}

async fn build(snapshot: DocumentOutboxSnapshot, storage: &KeyedStorage) -> Built {
    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentIndexingService::new(
        DocumentOutboxIndexer::new(
            QueuedReader::new(snapshot),
            config(),
            runtime.clone(),
            MemoryReceipts::default(),
        )
        .with_body_extractor(Arc::new(DocumentBodyExtractor::new(
            source_id(),
            storage.clone(),
            InProcessExtractor::new(Mode::Honest),
            production_registry(),
        ))),
    );
    let started = Instant::now();
    let outcome = indexer.handle(event(1)).await.unwrap();
    let millis = started.elapsed().as_millis();
    let IndexingOutcome::Published(key) = outcome else {
        panic!("expected publication, got {outcome:?}");
    };
    Built {
        runtime,
        key,
        millis,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str, default: &str| {
        args.iter()
            .position(|item| item == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let case = arg("--case", "text");
    let mib: usize = arg("--mib", "1").parse().unwrap();
    let queries: usize = arg("--queries", "200").parse().unwrap();

    let storage = KeyedStorage::default();
    let mut template = snapshot("measure", Vec::new());
    let base = template.live.remove(0);
    let mut raw_bytes = 0usize;
    for (document, parts) in corpus(&case, mib).into_iter().enumerate() {
        let mut record = base.clone();
        let version = DocumentVersionId::from_uuid(Uuid::from_u128(1_000_000 + document as u128));
        record.snapshot.document_id =
            DocumentId::from_uuid(Uuid::from_u128(2_000_000 + document as u128));
        record.snapshot.document_version_id = version;
        record.snapshot.current_version_id = Some(version);
        for (ordinal, (bytes, media)) in parts.into_iter().enumerate() {
            raw_bytes += bytes.len();
            let key = format!("objects/{document}/{ordinal}");
            storage.put(&key, &bytes);
            let seed = 10_000_000 + (document as u128) * 1_000 + (ordinal as u128) * 4;
            record.authoritative_items.push(binding(
                &key,
                &bytes,
                media,
                u32::try_from(ordinal).unwrap(),
                seed,
            ));
        }
        template.live.push(record);
    }

    // Cold: first build in a fresh process. Warm: a second full rebuild.
    let cold = build(template.clone(), &storage).await;
    let warm = build(template, &storage).await;
    let body: PublishedBody = warm.runtime.published_body(warm.key).unwrap().unwrap();
    let mut outcomes: Map<String, usize> = Map::new();
    let (mut units, mut unit_bytes) = (0usize, 0usize);
    for entry in &body.manifest.entries {
        let label = format!("{:?}/{:?}", entry.operation, entry.coverage);
        *outcomes.entry(label).or_default() += 1;
        units += entry.units.len();
        unit_bytes += entry
            .units
            .iter()
            .map(|unit| unit.text.len())
            .sum::<usize>();
    }

    let versions = body
        .manifest
        .entries
        .iter()
        .map(|entry| entry.version.resource_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let request = request();
    let reader = warm.runtime.lexical_reader();
    let mut latencies = Vec::with_capacity(queries);
    let (mut verified, mut filled, mut wanted) = (0usize, 0usize, 0usize);
    for index in 0..queries {
        let literal = if index % 2 == 0 {
            format!("東京{}", index % 50)
        } else {
            "共通".to_owned()
        };
        let started = Instant::now();
        let batch = reader
            .retrieve_body(warm.key, &request, &LexicalQuery::body_only(&literal, 10))
            .await
            .unwrap();
        latencies.push(started.elapsed().as_micros());
        verified += batch
            .hits
            .iter()
            .filter(|hit| hit.unit_hit.is_some())
            .count();
        filled += batch.hits.len();
        wanted += 10.min(versions);
    }
    latencies.sort_unstable();
    println!(
        "{}",
        serde_json::json!({
            "case": case,
            "mib": mib,
            "versions": versions,
            "items": body.manifest.entries.len(),
            "raw_bytes": raw_bytes,
            "outcomes": outcomes,
            "units": units,
            "unit_text_bytes": unit_bytes,
            "cold_build_ms": cold.millis,
            "warm_build_ms": warm.millis,
            "queries": queries,
            "query_us": {
                "p50": percentile(&latencies, 50),
                "p95": percentile(&latencies, 95),
                "p99": percentile(&latencies, 99),
            },
            "verified_hit_ratio": if filled == 0 { 0.0 } else { verified as f64 / filled as f64 },
            "unique_parent_fill": if wanted == 0 { 0.0 } else { filled as f64 / wanted as f64 },
        })
    );
}
