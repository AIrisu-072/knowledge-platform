//! P1-E02 BodyOnly retrieval: Unit documents only, unique parent refill and
//! exact literal spans.

use std::collections::BTreeMap;

use search_application::SearchError;
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
use search_tantivy::{LexicalBuildInput, LexicalDocument, TantivyLexicalIndex};
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

fn resource(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn key() -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(7)),
    }
}

fn manifest() -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: key().generation_id,
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: "snapshot-body".into(),
        resource_count: 8,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: "digest-7".into(),
        built_at: OffsetDateTime::from_unix_timestamp(100).unwrap(),
    }
}

fn source() -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        source_id(),
        "local",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentResource,
    );
    source.discovery_modes.push(DiscoveryMode::LocalDirectory);
    source
        .discovery_modes
        .push(DiscoveryMode::LocalContentSearch);
    source
}

fn card(id: u128, title: &str) -> LexicalDocument {
    LexicalDocument {
        resource_ref: resource(id),
        kind: ResourceKind::Document,
        canonical_name: format!("文書{id}"),
        title: Some(title.into()),
        aliases: Vec::new(),
        high_signal_text: None,
        body: None,
        locator: Some(format!("document:{id}")),
    }
}

fn profile() -> ExtractionProfileId {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 512);
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

fn unit(parent: u128, part: &str, ordinal: u32, text: &str) -> KnowledgeUnit {
    let version = ResourceVersionRef {
        source_id: source_id(),
        resource_id: resource(parent),
        source_native_version: "version-1".into(),
    };
    let part = ContentPartRef {
        source_native_part_id: format!("{part}-{parent}"),
        logical_path: format!("本文/{part}"),
        ordinal: u32::from(part != "primary"),
    };
    let locator = NativeLocator::Text {
        line_start: ordinal,
        line_end: ordinal + 1,
    };
    let profile = profile();
    KnowledgeUnit {
        unit_id: UnitId::derive(&version, &part, &profile, &locator, ordinal).unwrap(),
        version,
        part,
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
        },
    }
}

fn build(documents: Vec<LexicalDocument>, units: Vec<KnowledgeUnit>) -> TantivyLexicalIndex {
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            manifest(),
            &source(),
            LexicalBuildInput::new(source_id(), "snapshot-body", "schema-1", 1, documents)
                .with_body_units(units),
        )
        .unwrap();
    index
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
async fn body_only_excludes_title_and_resource_doc() {
    let index = build(
        vec![card(10, "規程"), card(11, "手順")],
        vec![unit(11, "primary", 0, "本文 規程")],
    );
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("規程", 10))
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert!(batch.exhausted_matching_units);
    let hit = &batch.hits[0];
    assert_eq!(hit.candidate.resource_ref, Some(resource(11)));
    assert_eq!(
        hit.candidate.candidate_id,
        format!("{}:{}", source_id().as_uuid(), resource(11).as_uuid())
    );
    let unit_hit = hit.unit_hit.as_ref().unwrap();
    assert_eq!(unit_hit.parent_resource, resource(11));
    assert_eq!(unit_hit.generation, key());
    assert_eq!((unit_hit.span.start_byte, unit_hit.span.end_byte), (7, 13));

    // ExistingFields still ranks the title and never reads Unit text.
    let titles = index
        .retrieve(key(), &request(), &LexicalQuery::new("規程", 10))
        .await
        .unwrap();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0].resource_ref, Some(resource(10)));

    // Each method refuses the other's field scope.
    assert!(matches!(
        index
            .retrieve_body(key(), &request(), &LexicalQuery::new("規程", 10))
            .await,
        Err(SearchError::InvalidRequest(_))
    ));

    // A schema-1 generation is not body-ready and cannot answer BodyOnly.
    let legacy = TantivyLexicalIndex::new();
    legacy
        .build_generation(
            manifest(),
            &source(),
            LexicalBuildInput::new(
                source_id(),
                "snapshot-body",
                "schema-1",
                1,
                vec![card(10, "規程")],
            ),
        )
        .unwrap();
    assert!(matches!(
        legacy
            .retrieve_body(key(), &request(), &LexicalQuery::body_only("規程", 10))
            .await,
        Err(SearchError::SourceUnavailable(_))
    ));
}

#[tokio::test]
async fn unique_parent_refill_and_underfill() {
    let dense = |count: u32| {
        let mut units: Vec<_> = (0..count)
            .map(|ordinal| unit(10, "primary", ordinal, "規程 規程 規程"))
            .collect();
        units.push(unit(
            11,
            "primary",
            0,
            "規程 は 長い 文書 の 一部 です 説明 説明 説明 説明",
        ));
        units
    };
    // Forty Units of one parent fill the first window; refill reaches the other parent.
    let index = build(vec![card(10, "甲"), card(11, "乙")], dense(40));
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("規程", 2))
        .await
        .unwrap();
    let parents: Vec<_> = batch
        .hits
        .iter()
        .map(|hit| hit.candidate.resource_ref)
        .collect();
    assert_eq!(parents, vec![Some(resource(10)), Some(resource(11))]);
    assert!(batch.hits.iter().all(|hit| hit.unit_hit.is_some()));
    assert!(batch.exhausted_matching_units);
    // The representative is deterministic: best score, then the lowest ordinal.
    assert_eq!(batch.hits[0].unit_hit.as_ref().unwrap().span.start_byte, 0);

    // Past the bounded window the second parent is unreachable: underfill, not exhaustion.
    let index = build(vec![card(10, "甲"), card(11, "乙")], dense(200));
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("規程", 2))
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert!(!batch.exhausted_matching_units);

    // A limit that truncates matching parents is not exhaustion either.
    let index = build(vec![card(10, "甲"), card(11, "乙")], dense(3));
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("規程", 1))
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert!(!batch.exhausted_matching_units);
}

#[tokio::test]
async fn same_text_separate_part_keeps_ref() {
    let units = vec![
        unit(10, "primary", 0, "同じ 本文"),
        unit(10, "attachment", 0, "同じ 本文"),
        unit(11, "primary", 0, "同じ 本文"),
    ];
    let index = build(vec![card(10, "甲"), card(11, "乙")], units.clone());
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("同じ 本文", 10))
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 2);
    assert!(batch.exhausted_matching_units);
    for hit in &batch.hits {
        let unit_hit = hit.unit_hit.as_ref().unwrap();
        let source_unit = units
            .iter()
            .find(|unit| unit.unit_id == unit_hit.unit_id)
            .unwrap();
        assert_eq!(Some(unit_hit.parent_resource), hit.candidate.resource_ref);
        assert_eq!(unit_hit.version, source_unit.version);
        assert_eq!(unit_hit.part, source_unit.part);
        assert_eq!(unit_hit.raw, source_unit.provenance.raw);
        assert_eq!(unit_hit.profile, source_unit.provenance.profile);
        assert_eq!(unit_hit.text_sha256, source_unit.text_sha256);
        assert_eq!(
            unit_hit.authoritative_representation_ref,
            source_unit.provenance.authoritative_representation_ref
        );
        let encoded: String = source_unit
            .locator
            .encode()
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(unit_hit.opaque_locator, encoded);
    }
    // The same parent's two Parts collapse to one deterministic representative.
    let first = batch.hits[0].unit_hit.as_ref().unwrap();
    let again = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("同じ 本文", 10))
        .await
        .unwrap();
    assert_eq!(again.hits[0].unit_hit.as_ref(), Some(first));
}

#[tokio::test]
async fn token_hit_without_literal_span_is_not_qualified() {
    let index = build(
        vec![card(10, "甲"), card(11, "乙")],
        vec![
            unit(10, "primary", 0, "Tokyo Office"),
            unit(11, "primary", 0, "東京都の規程"),
        ],
    );
    // The analyzer lowercases tokens; the literal comparison does not.
    let batch = index
        .retrieve_body(
            key(),
            &request(),
            &LexicalQuery::body_only("tokyo office", 10),
        )
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].candidate.resource_ref, Some(resource(10)));
    assert!(batch.hits[0].unit_hit.is_none());

    // A substring inside one analyzer token is a token no-hit, which proves nothing.
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("東京", 10))
        .await
        .unwrap();
    assert!(batch.hits.is_empty());

    // Text without any searchable token cannot be enumerated.
    let batch = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("。、", 10))
        .await
        .unwrap();
    assert!(batch.hits.is_empty());
    assert!(!batch.exhausted_matching_units);
}

/// Japanese text is segmented only by the CJK bigram analyzer: an unspaced
/// sentence is one token under the legacy default, so a word inside it is
/// found only with bigrams. The persisted generation keeps its analyzer.
#[tokio::test]
async fn cjk_bigram_analyzer_finds_words_inside_unspaced_japanese() {
    let bigram = |dir: Option<&std::path::Path>| {
        let mut manifest = manifest();
        manifest.analyzer_version = Some(search_tantivy::CJK_BIGRAM_ANALYZER_VERSION.into());
        let input = LexicalBuildInput::new(
            source_id(),
            "snapshot-body",
            "schema-1",
            1,
            vec![card(10, "アンパサンドの歴史"), card(11, "会議室の予約手順")],
        )
        .with_body_units(vec![unit(
            11,
            "primary",
            0,
            "会議室は前日までに総務部へ申請する。",
        )])
        .with_analyzer_version(search_tantivy::CJK_BIGRAM_ANALYZER_VERSION);
        let index = TantivyLexicalIndex::new();
        match dir {
            Some(dir) => index.build_generation_at(manifest, &source(), input, dir),
            None => index.build_generation(manifest, &source(), input),
        }
        .unwrap();
        index
    };
    let index = bigram(None);
    let titles = index
        .retrieve(key(), &request(), &LexicalQuery::new("パサンド", 10))
        .await
        .unwrap();
    assert_eq!(titles[0].resource_ref, Some(resource(10)));
    let body = index
        .retrieve_body(
            key(),
            &request(),
            &LexicalQuery::body_only("総務部へ申請", 10),
        )
        .await
        .unwrap();
    assert_eq!(body.hits.len(), 1);
    assert_eq!(body.hits[0].candidate.resource_ref, Some(resource(11)));
    // A substring that is not contiguous in the text is not a phrase match.
    let scattered = index
        .retrieve_body(key(), &request(), &LexicalQuery::body_only("申請総務", 10))
        .await
        .unwrap();
    assert!(scattered.hits.is_empty());

    // The legacy default analyzer cannot see the word inside the sentence.
    let legacy = build(
        vec![card(11, "会議室の予約手順")],
        vec![unit(
            11,
            "primary",
            0,
            "会議室は前日までに総務部へ申請する。",
        )],
    );
    let missed = legacy
        .retrieve_body(
            key(),
            &request(),
            &LexicalQuery::body_only("総務部へ申請", 10),
        )
        .await
        .unwrap();
    assert!(missed.hits.is_empty());

    // A persisted bigram generation reopens with its own analyzer.
    let dir = std::env::temp_dir().join(format!("kp-bigram-{}", Uuid::new_v4()));
    bigram(Some(&dir));
    let mut manifest = manifest();
    manifest.analyzer_version = Some(search_tantivy::CJK_BIGRAM_ANALYZER_VERSION.into());
    let reopened = TantivyLexicalIndex::new();
    let persisted = reopened
        .load_generation_at(&manifest, &source(), &dir)
        .unwrap();
    assert_eq!(
        persisted.analyzer_version,
        search_tantivy::CJK_BIGRAM_ANALYZER_VERSION
    );
    let again = reopened
        .retrieve_body(
            key(),
            &request(),
            &LexicalQuery::body_only("総務部へ申請", 10),
        )
        .await
        .unwrap();
    assert_eq!(again.hits.len(), 1);
    // The manifest and the persisted sidecar must name the same analyzer.
    let legacy_manifest = self::manifest();
    assert!(TantivyLexicalIndex::inspect_persisted(&legacy_manifest, &source(), &dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
