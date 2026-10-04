use document_domain::{DocumentId, DocumentVersionId, FolderId, LifecycleState, Title};
use search_application::ports::{LexicalQuery, LexicalRetrieverPort};
use search_application::projection::{ProjectionCompiler, ProjectionError};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, ResourceVersionId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::profile::{DiscoveryLens, FacetState};
use search_core::projection::ProjectionGenerationManifest;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_source_document::{
    DocumentAccessProjectionInput, DocumentSourceSnapshot, DocumentSourceTranslation,
    DocumentSourceTranslator, DocumentVisibilityClass, DsiEvidenceRefs, PermittedDocumentMetadata,
    PublicationEndRecord, TranslationError,
};
use search_tantivy::{LexicalIndexError, TantivyLexicalIndex};
use time::OffsetDateTime;
use uuid::Uuid;

fn at(second: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(second).unwrap()
}

fn document_id(raw: u128) -> DocumentId {
    DocumentId::from_uuid(Uuid::from_u128(raw))
}

fn version_id(raw: u128) -> DocumentVersionId {
    DocumentVersionId::from_uuid(Uuid::from_u128(raw))
}

fn translator() -> DocumentSourceTranslator {
    translator_with_fields(vec!["title".into(), "permitted_metadata".into()])
}

fn translator_with_fields(searchable_fields: Vec<String>) -> DocumentSourceTranslator {
    let source_id = SourceId::from_uuid(Uuid::from_u128(70));
    let mut source = DiscoverableSource::new(
        source_id,
        "document-platform",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    source.resource_types.push(ResourceKind::Knowledge);
    source.access_model = Some("document-current-check".into());
    let lens = DiscoveryLens {
        lens_id: "document-version".into(),
        lens_version: 1,
        resource_type: ResourceKind::Knowledge,
        domain_scope: None,
        source_scope: Some(source_id),
        identity_fields: vec!["document_version_id".into()],
        high_signal_facets: vec!["document_type".into(), "category".into()],
        searchable_fields,
        applicability_fields: vec![],
        temporal_fields: vec!["effective_from".into(), "effective_to".into()],
        relation_fields: vec![],
        extraction_policy: None,
        projection_policy: None,
    };
    DocumentSourceTranslator::new(source, lens, "schema-1", "registry-1")
}

#[test]
fn lexical_fields_outside_title_and_permitted_metadata_are_rejected() {
    let translator = translator_with_fields(vec!["title".into(), "dsi_evidence".into()]);
    assert_eq!(
        translator.translate(snapshot(20)).unwrap_err(),
        TranslationError::UnsupportedLexicalField
    );
}

#[tokio::test]
async fn unselected_permitted_metadata_is_absent_from_lexical_index() {
    let translated = translator_with_fields(vec!["title".into()])
        .translate(snapshot(20))
        .unwrap();
    let inputs = translated.live_inputs().unwrap();
    let generation = manifest(inputs.projection.source.source_id, "document-snapshot-17");
    let lexical = TantivyLexicalIndex::new();
    lexical
        .build_generation(
            generation.clone(),
            &inputs.projection.source,
            inputs.lexical.clone(),
        )
        .unwrap();
    let hits = lexical
        .retrieve(generation.key(), &request(), &LexicalQuery::new("人事", 10))
        .await
        .unwrap();
    assert_eq!(
        (
            inputs.lexical_document.high_signal_text.as_deref(),
            hits.len()
        ),
        (None, 0)
    );
}

fn snapshot(version: u128) -> DocumentSourceSnapshot {
    DocumentSourceSnapshot {
        source_snapshot: "document-snapshot-17".into(),
        document_id: document_id(10),
        document_version_id: version_id(version),
        current_version_id: Some(version_id(version)),
        publication_end: None,
        lifecycle_state: LifecycleState::Published,
        title: Title::new("採用規程").unwrap(),
        metadata: PermittedDocumentMetadata {
            document_type: Some("規程".into()),
            category: Some("人事".into()),
        },
        folder_id: FolderId::from_uuid(Uuid::from_u128(30)),
        created_at: at(100),
        published_at: Some(at(200)),
        withdrawn_at: None,
        effective_from: Some(at(250)),
        effective_to: Some(at(500)),
        access: DocumentAccessProjectionInput {
            access_scope: Some("opaque-scope".into()),
        },
        dsi: Some(DsiEvidenceRefs {
            capability_refs: vec!["supports-structure".into()],
            evidence_refs: vec!["DSI_ONLY_SENTINEL".into()],
        }),
    }
}

fn manifest(source_id: SourceId, source_snapshot: &str) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(71)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: source_snapshot.into(),
        resource_count: 1,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: "synthetic-digest".into(),
        built_at: at(300),
    }
}

fn request() -> DiscoveryRequest {
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(72)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: Vec::new(),
            authority_requirements: Vec::new(),
            freshness_requirements: Vec::new(),
            constraints: Vec::new(),
            completion_requirement: EvidenceRequirement::new(Vec::new()),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(73)),
            at(300),
            at(300),
            "UTC",
        ),
        access_context: "synthetic-actor".into(),
    }
}

#[test]
fn current_published_version_maps_identity_and_safe_source_fields() {
    let translated = translator().translate(snapshot(20)).unwrap();
    let DocumentSourceTranslation::Live(inputs) = &translated else {
        panic!("current published version must use the Live route");
    };
    let input = &inputs.projection;
    assert_eq!(
        translated.visibility(),
        DocumentVisibilityClass::CurrentPublished
    );
    assert!(translated.live_inputs().is_some());
    assert_eq!(
        input.resource.identity.resource_id,
        ResourceId::from_uuid(Uuid::from_u128(20))
    );
    assert_eq!(
        input.resource.identity.resource_version,
        Some(ResourceVersionId::from_uuid(Uuid::from_u128(20)))
    );
    assert_eq!(
        input.resource.identity.source_native_id.as_deref(),
        Some(document_id(10).as_uuid().to_string().as_str())
    );
    assert_eq!(input.title.as_deref(), Some("採用規程"));
    assert_eq!(
        input.resource.identity.access_scope.as_deref(),
        Some("opaque-scope")
    );
    assert_eq!(
        input.source.access_model.as_deref(),
        Some("document-current-check")
    );
    assert_eq!(
        input.resource.temporal_profile.effective_from,
        Some(at(250))
    );
    assert_eq!(input.resource.temporal_profile.effective_to, Some(at(500)));
    assert_eq!(
        input.typed_facets.get("document_type"),
        Some(&FacetState::Known(TypedValue::String("規程".into())))
    );
    assert_eq!(
        input.typed_facets.get("category"),
        Some(&FacetState::Known(TypedValue::String("人事".into())))
    );
    assert!(input.assertions.is_empty());
    assert!(input.authority_resolutions.is_empty());
    assert_eq!(
        inputs.dsi.as_ref().unwrap().evidence_refs,
        vec!["DSI_ONLY_SENTINEL"]
    );
    assert_eq!(
        inputs.lexical_document.resource_ref,
        input.resource.identity.resource_id
    );
    assert!(inputs.lexical_document.body.is_none());
}

#[test]
fn version_identity_is_distinct_when_document_id_is_shared() {
    let first = translator().translate(snapshot(20)).unwrap();
    let second = translator().translate(snapshot(21)).unwrap();
    let first = first.live_inputs().unwrap();
    let second = second.live_inputs().unwrap();
    assert_ne!(
        first.projection.resource.identity.resource_id,
        second.projection.resource.identity.resource_id
    );
    assert_ne!(
        first.projection.resource.identity.resource_version,
        second.projection.resource.identity.resource_version
    );
    assert_eq!(
        first.projection.resource.identity.source_native_id,
        second.projection.resource.identity.source_native_id
    );
}

#[test]
fn lifecycle_classes_cannot_enter_live_route() {
    let translator = translator();
    let mut historical = snapshot(20);
    historical.current_version_id = Some(version_id(21));
    let historical = translator.translate(historical).unwrap();
    assert_eq!(
        historical.visibility(),
        DocumentVisibilityClass::HistoricalPublished
    );
    assert!(matches!(
        &historical,
        DocumentSourceTranslation::Historical { .. }
    ));
    assert!(historical.live_inputs().is_none());
    assert!(historical.historical_inputs().is_some());

    let mut no_current_but_not_t10 = snapshot(20);
    no_current_but_not_t10.current_version_id = None;
    let no_current_but_not_t10 = translator.translate(no_current_but_not_t10).unwrap();
    assert_eq!(
        no_current_but_not_t10.visibility(),
        DocumentVisibilityClass::HistoricalPublished
    );
    assert!(no_current_but_not_t10.live_inputs().is_none());

    let mut withdrawn = snapshot(20);
    withdrawn.lifecycle_state = LifecycleState::Withdrawn;
    withdrawn.current_version_id = None;
    withdrawn.withdrawn_at = Some(at(350));
    let withdrawn = translator.translate(withdrawn).unwrap();
    assert_eq!(withdrawn.visibility(), DocumentVisibilityClass::Withdrawn);
    assert!(matches!(
        &withdrawn,
        DocumentSourceTranslation::Historical { .. }
    ));
    assert!(withdrawn.live_inputs().is_none());

    let mut ended = snapshot(20);
    ended.current_version_id = None;
    ended.publication_end = Some(PublicationEndRecord {
        operation_id: Uuid::from_u128(99),
        ended_at: at(400),
    });
    let ended = translator.translate(ended).unwrap();
    assert_eq!(
        ended.visibility(),
        DocumentVisibilityClass::PublicationEnded
    );
    assert!(matches!(
        &ended,
        DocumentSourceTranslation::Historical { .. }
    ));
    assert!(ended.live_inputs().is_none());
    assert_eq!(
        ended
            .historical_inputs()
            .unwrap()
            .publication_end
            .as_ref()
            .unwrap()
            .operation_id,
        Uuid::from_u128(99)
    );

    let mut working = snapshot(20);
    working.lifecycle_state = LifecycleState::Working;
    working.current_version_id = None;
    working.published_at = None;
    let working = translator.translate(working).unwrap();
    assert_eq!(
        working.visibility(),
        DocumentVisibilityClass::WorkingAuthoring
    );
    assert!(matches!(
        &working,
        DocumentSourceTranslation::Authoring { .. }
    ));
    assert!(working.live_inputs().is_none());
    assert!(working.authoring_inputs().is_some());
}

#[test]
fn t10_working_residue_stays_on_authoring_route() {
    let mut input = snapshot(20);
    input.lifecycle_state = LifecycleState::Working;
    input.current_version_id = None;
    input.published_at = None;
    input.publication_end = Some(PublicationEndRecord {
        operation_id: Uuid::from_u128(99),
        ended_at: at(400),
    });
    let translated = translator().translate(input).unwrap();
    assert_eq!(
        translated.visibility(),
        DocumentVisibilityClass::PublicationEnded
    );
    assert!(matches!(
        &translated,
        DocumentSourceTranslation::Authoring { .. }
    ));
    assert!(translated.live_inputs().is_none());
}

#[test]
fn contradictory_source_states_fail_closed() {
    let translator = translator();
    let mut ended_with_current = snapshot(20);
    ended_with_current.publication_end = Some(PublicationEndRecord {
        operation_id: Uuid::from_u128(99),
        ended_at: at(400),
    });
    assert_eq!(
        translator.translate(ended_with_current).unwrap_err(),
        TranslationError::PublicationEndedWithCurrent
    );

    let mut current_working = snapshot(20);
    current_working.lifecycle_state = LifecycleState::Working;
    current_working.published_at = None;
    assert_eq!(
        translator.translate(current_working).unwrap_err(),
        TranslationError::CurrentVersionNotPublished
    );

    let mut published_without_timestamp = snapshot(20);
    published_without_timestamp.published_at = None;
    assert_eq!(
        translator
            .translate(published_without_timestamp)
            .unwrap_err(),
        TranslationError::MissingPublishedAt
    );

    let mut published_with_withdrawal = snapshot(20);
    published_with_withdrawal.withdrawn_at = Some(at(350));
    assert_eq!(
        translator.translate(published_with_withdrawal).unwrap_err(),
        TranslationError::InconsistentLifecycle
    );
}

#[tokio::test]
async fn projection_and_lexical_use_one_snapshot_and_never_index_dsi_as_body() {
    let translated = translator().translate(snapshot(20)).unwrap();
    let inputs = translated.live_inputs().unwrap();
    let expected_manifest = manifest(inputs.projection.source.source_id, "document-snapshot-17");
    ProjectionCompiler::compile_resource(&expected_manifest, &inputs.projection).unwrap();
    let lexical = TantivyLexicalIndex::new();
    lexical
        .build_generation(
            expected_manifest.clone(),
            &inputs.projection.source,
            inputs.lexical.clone(),
        )
        .unwrap();
    let request = request();
    for permitted in ["採用規程", "人事"] {
        assert_eq!(
            lexical
                .retrieve(
                    expected_manifest.key(),
                    &request,
                    &LexicalQuery::new(permitted, 10)
                )
                .await
                .unwrap()
                .len(),
            1
        );
    }
    assert!(
        lexical
            .retrieve(
                expected_manifest.key(),
                &request,
                &LexicalQuery::new("DSI_ONLY_SENTINEL", 10),
            )
            .await
            .unwrap()
            .is_empty()
    );

    let other_snapshot = manifest(inputs.projection.source.source_id, "document-snapshot-18");
    assert_eq!(
        ProjectionCompiler::compile_resource(&other_snapshot, &inputs.projection).unwrap_err(),
        ProjectionError::SourceSnapshotMismatch
    );
    assert!(matches!(
        TantivyLexicalIndex::new().build_generation(
            other_snapshot,
            &inputs.projection.source,
            inputs.lexical.clone()
        ),
        Err(LexicalIndexError::SourceSnapshotMismatch)
    ));
}
