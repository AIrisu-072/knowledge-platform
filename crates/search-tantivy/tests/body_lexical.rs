//! P1-E01 body-ready (schema-2) lexical generations and Unit document enumeration.

use std::collections::BTreeMap;

use search_application::ports::{LexicalQuery, LexicalRetrieverPort};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    FormatSettings, KnowledgeUnit, NativeLocator, RawBinding, ResourceVersionRef, UnitId, UnitKind,
    UnitProvenance, text_sha256,
};
use search_core::observation::Coverage;
use search_core::projection::{ProjectionGenerationKey, ProjectionGenerationManifest};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_tantivy::{
    LexicalBuildInput, LexicalDocument, LexicalIndexError, TantivyLexicalIndex,
    lexical_input_digest,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

fn resource(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn key(generation: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
    }
}

fn manifest(generation: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: "snapshot-body".into(),
        resource_count: 4,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("digest-{generation}"),
        built_at: OffsetDateTime::from_unix_timestamp(100).unwrap(),
    }
}

fn source(retention: RetentionMode) -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        source_id(),
        "local",
        EnumerationSemantics::Complete,
        retention,
    );
    source.discovery_modes.push(DiscoveryMode::LocalDirectory);
    source
        .discovery_modes
        .push(DiscoveryMode::LocalContentSearch);
    source
}

fn card(id: u128, name: &str) -> LexicalDocument {
    LexicalDocument {
        resource_ref: resource(id),
        kind: ResourceKind::Document,
        canonical_name: name.into(),
        title: None,
        aliases: Vec::new(),
        high_signal_text: None,
        body: None,
        locator: None,
    }
}

fn profile() -> ExtractionProfileId {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 16);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1024);
    limits.insert(BudgetKey::WorkerOutputBytes, 65_536);
    ExtractionProfileId::for_definition(&ExtractionProfileDefinitionV1 {
        format: FormatId::Text,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [1; 32],
        native_binary_sha256: None,
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: FormatSettings::Text {
            charset: "utf-8".into(),
        },
        limits,
    })
    .unwrap()
}

fn unit(source: SourceId, parent: u128, ordinal: u32, text: &str) -> KnowledgeUnit {
    let version = ResourceVersionRef {
        source_id: source,
        resource_id: resource(parent),
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: format!("part-{parent}"),
        logical_path: "本文/primary".into(),
        ordinal: 0,
    };
    let locator = NativeLocator::Text {
        line_start: ordinal,
        line_end: ordinal + 1,
    };
    let profile = profile();
    KnowledgeUnit {
        unit_id: UnitId::derive(&version, &part, &profile, &locator, ordinal).unwrap(),
        version: version.into(),
        part: part.into(),
        parent_unit_id: None,
        ordinal,
        kind: UnitKind::PlainText,
        text: text.into(),
        locator,
        text_sha256: text_sha256(text),
        provenance: UnitProvenance {
            source_snapshot: "snapshot-body".into(),
            authoritative_representation_ref: format!("representation-{parent}"),
            raw: RawBinding {
                sha256: [9; 32],
                size_bytes: 64,
                media_type: "text/plain".into(),
            },
            detected_format: FormatId::Text,
            archive_inner_format: None,
            profile,
            parser_build_id: "search-extraction-worker-test".into(),
        }
        .into(),
    }
}

fn input(documents: Vec<LexicalDocument>, units: Option<Vec<KnowledgeUnit>>) -> LexicalBuildInput {
    let input = LexicalBuildInput::new(source_id(), "snapshot-body", "schema-1", 1, documents);
    match units {
        Some(units) => input.with_body_units(units),
        None => input,
    }
}

fn units() -> Vec<KnowledgeUnit> {
    vec![
        unit(source_id(), 10, 0, "東京の本文"),
        unit(source_id(), 10, 1, "同文。"),
        unit(source_id(), 11, 0, "大阪の本文"),
    ]
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(1)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: Vec::new(),
            required_claims: Vec::new(),
            authority_requirements: Vec::new(),
            freshness_requirements: Vec::new(),
            constraints: Vec::new(),
            completion_requirement: EvidenceRequirement::new(Vec::new()),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(2)),
            now,
            now,
            "UTC",
        ),
        access_context: "test-principal".into(),
    }
}

#[tokio::test]
async fn schema2_resource_and_unit_fields() {
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            manifest(7),
            &source(RetentionMode::PersistentResource),
            input(vec![card(10, "規程"), card(11, "手順")], Some(units())),
        )
        .unwrap();
    assert!(index.is_body_ready(key(7)).unwrap());
    let docs = index.enumerate_unit_docs(key(7)).unwrap();
    assert_eq!(docs.len(), 3);
    let mut expected = units();
    expected.sort_by_key(|unit| unit.unit_id);
    for (doc, unit) in docs.iter().zip(&expected) {
        assert_eq!(doc.generation, key(7));
        assert_eq!(doc.parent_resource, unit.version.resource_id);
        assert_eq!(doc.unit_id, unit.unit_id);
        assert_eq!(doc.text, unit.text);
        assert_eq!(doc.text_sha256, unit.text_sha256);
        assert_eq!(doc.locator, unit.locator);
        assert_eq!(doc.part, *unit.part);
        assert_eq!(doc.raw, unit.provenance.raw);
        assert_eq!(doc.profile, unit.provenance.profile);
        assert_eq!(doc.kind, UnitKind::PlainText);
    }
    // Unit text lives only in the typed Unit index; Resource retrieval is unchanged.
    let hits = index
        .retrieve(key(7), &request(), &LexicalQuery::new("東京の本文", 10))
        .await
        .unwrap();
    assert!(hits.is_empty());
    let hits = index
        .retrieve(key(7), &request(), &LexicalQuery::new("規程", 10))
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn schema1_generation_stays_readable_but_not_body_ready() {
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            manifest(8),
            &source(RetentionMode::PersistentResource),
            input(vec![card(10, "規程")], None),
        )
        .unwrap();
    assert!(!index.is_body_ready(key(8)).unwrap());
    assert!(matches!(
        index.enumerate_unit_docs(key(8)),
        Err(LexicalIndexError::NotBodyReady)
    ));
    // A body-ready generation whose items had no text has zero Unit documents.
    index
        .build_generation(
            manifest(9),
            &source(RetentionMode::PersistentResource),
            input(vec![card(10, "規程")], Some(Vec::new())),
        )
        .unwrap();
    assert_eq!(index.enumerate_unit_docs(key(9)).unwrap(), Vec::new());
}

#[test]
fn unit_documents_must_belong_to_the_generation() {
    let cases = [
        (
            vec![unit(SourceId::from_uuid(Uuid::from_u128(99)), 10, 0, "x")],
            RetentionMode::PersistentResource,
            "source",
        ),
        (
            vec![unit(source_id(), 12, 0, "x")],
            RetentionMode::PersistentResource,
            "parent",
        ),
        (
            vec![unit(source_id(), 10, 0, "x"), unit(source_id(), 10, 0, "x")],
            RetentionMode::PersistentResource,
            "duplicate",
        ),
        (
            vec![unit(source_id(), 10, 0, "x")],
            RetentionMode::PersistentDiscoveryMetadata,
            "retention",
        ),
    ];
    for (generation, (units, retention, case)) in cases.into_iter().enumerate() {
        let index = TantivyLexicalIndex::new();
        let result = index.build_generation(
            manifest(20 + generation as u128),
            &source(retention),
            input(vec![card(10, "規程")], Some(units)),
        );
        let matched = match case {
            "source" => matches!(result, Err(LexicalIndexError::UnitSourceMismatch)),
            "parent" => matches!(result, Err(LexicalIndexError::UnitParentMissing)),
            "duplicate" => matches!(result, Err(LexicalIndexError::DuplicateUnit)),
            _ => matches!(result, Err(LexicalIndexError::BodyNotPermitted)),
        };
        assert!(matched, "{case}: {result:?}");
    }
}

#[test]
fn lexical_input_digest_is_deterministic_and_covers_body_text() {
    let documents = vec![card(10, "規程"), card(11, "手順")];
    let mut reversed = documents.clone();
    reversed.reverse();
    let mut shuffled_units = units();
    shuffled_units.reverse();
    let first = lexical_input_digest(&input(documents.clone(), Some(units()))).unwrap();
    let second = lexical_input_digest(&input(reversed, Some(shuffled_units))).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.count, 5);
    let mut changed = units();
    changed[0].text = "別の本文".into();
    changed[0].text_sha256 = text_sha256("別の本文");
    let third = lexical_input_digest(&input(documents.clone(), Some(changed))).unwrap();
    assert_ne!(first.digest, third.digest);
    let schema_one = lexical_input_digest(&input(documents, None)).unwrap();
    assert_ne!(first.digest, schema_one.digest);
}

#[cfg(feature = "fault-injection")]
#[test]
fn enumerated_docs_reflect_actual_index_not_builder_inputs() {
    use search_tantivy::UnitIndexFault;

    for (generation, fault, expected) in [
        (30, UnitIndexFault::DropFirst, 2),
        (31, UnitIndexFault::DuplicateFirst, 4),
        (32, UnitIndexFault::ReplaceFirstText("差替え".into()), 3),
    ] {
        let index = TantivyLexicalIndex::new();
        index
            .build_generation(
                manifest(generation),
                &source(RetentionMode::PersistentResource),
                input(vec![card(10, "規程"), card(11, "手順")], Some(units())),
            )
            .unwrap();
        index
            .inject_unit_fault(key(generation), fault.clone())
            .unwrap();
        let docs = index.enumerate_unit_docs(key(generation)).unwrap();
        assert_eq!(docs.len(), expected, "{fault:?}");
        if let UnitIndexFault::ReplaceFirstText(text) = fault {
            let replaced = docs.iter().find(|doc| doc.text == text).unwrap();
            assert_ne!(replaced.text_sha256, text_sha256(&text));
        }
    }
}
