use std::collections::BTreeMap;

use search_application::projection::{
    GenerationPublication, PersistableGenerationManifest, PersistableResourceProjection,
    ProjectionCompiler, ProjectionError, ProjectionInput, ProjectionPublishState,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::authority::{AuthorityConflict, AuthorityResolution};
use search_core::id::{ProjectionGenerationId, RelationId, ResourceId, SourceId};
use search_core::observation::Coverage;
use search_core::predicate::{DecimalValue, TypedValue};
use search_core::profile::{DiscoveryLens, DiscoveryProfile, FacetState};
use search_core::projection::{ProjectionGenerationKey, ProjectionGenerationManifest};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::{DiscoverableResource, ResourceBody, ResourceIdentity, ResourceKind};
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id(number: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(number))
}

fn generation_id(number: u128) -> ProjectionGenerationId {
    ProjectionGenerationId::from_uuid(Uuid::from_u128(number))
}

fn resource_id(number: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(number))
}

fn at(second: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(second).unwrap()
}

fn manifest(
    source_id: SourceId,
    generation_id: ProjectionGenerationId,
) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id,
        generation_id,
        projection_schema_version: "schema-4".into(),
        lens_version: 7,
        semantic_registry_version: "registry-12".into(),
        analyzer_version: Some("analyzer-2".into()),
        embedding_model_version: None,
        graph_schema_version: Some("graph-3".into()),
        source_snapshot: "snapshot-42".into(),
        resource_count: 1,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: "sha256:fixture".into(),
        built_at: at(1_000),
    }
}

fn input(source_id: SourceId, retention_mode: RetentionMode) -> ProjectionInput {
    let source = DiscoverableSource::new(
        source_id,
        "fixture",
        EnumerationSemantics::Complete,
        retention_mode,
    );
    let mut profile = DiscoveryProfile::new("canonical");
    profile.aliases.push("alias".into());
    profile
        .high_signal_facets
        .insert("known".into(), FacetState::Known("yes".into()));
    profile
        .high_signal_facets
        .insert("unknown".into(), FacetState::Unknown);
    profile
        .high_signal_facets
        .insert("na".into(), FacetState::NotApplicable);
    profile
        .high_signal_facets
        .insert("conflict".into(), FacetState::Conflict);
    let mut identity = ResourceIdentity::new(resource_id(100), ResourceKind::Knowledge, source_id);
    identity.access_scope = Some("team-a".into());
    identity.valid_from = Some(at(100));
    let resource = DiscoverableResource::new(identity, ResourceBody::Knowledge, profile);
    let lens = DiscoveryLens {
        lens_id: "knowledge-card".into(),
        lens_version: 7,
        resource_type: ResourceKind::Knowledge,
        domain_scope: None,
        source_scope: Some(source_id),
        identity_fields: vec![],
        high_signal_facets: vec!["known".into()],
        searchable_fields: vec![],
        applicability_fields: vec![],
        temporal_fields: vec![],
        relation_fields: vec![],
        extraction_policy: None,
        projection_policy: None,
    };
    ProjectionInput {
        resource,
        source,
        lens,
        source_snapshot: "snapshot-42".into(),
        projection_schema_version: "schema-4".into(),
        semantic_registry_version: "registry-12".into(),
        title: Some("display title".into()),
        typed_facets: BTreeMap::new(),
        assertions: vec![],
        authority_resolutions: BTreeMap::new(),
        relations: vec![],
    }
}

#[test]
fn compiler_rejects_mixed_snapshot_schema_or_registry_inputs() {
    let source = source_id(1);
    let manifest = manifest(source, generation_id(11));
    let mut wrong_snapshot = input(source, RetentionMode::PersistentDiscoveryMetadata);
    wrong_snapshot.source_snapshot = "snapshot-43".into();
    assert!(matches!(
        ProjectionCompiler::compile_resource(&manifest, &wrong_snapshot),
        Err(ProjectionError::SourceSnapshotMismatch)
    ));

    let mut wrong_schema = input(source, RetentionMode::PersistentDiscoveryMetadata);
    wrong_schema.projection_schema_version = "schema-5".into();
    assert!(matches!(
        ProjectionCompiler::compile_resource(&manifest, &wrong_schema),
        Err(ProjectionError::SchemaVersionMismatch)
    ));

    let mut wrong_registry = input(source, RetentionMode::PersistentDiscoveryMetadata);
    wrong_registry.semantic_registry_version = "registry-13".into();
    assert!(matches!(
        ProjectionCompiler::compile_resource(&manifest, &wrong_registry),
        Err(ProjectionError::SemanticRegistryVersionMismatch)
    ));
}

#[test]
fn compiler_binds_card_to_source_local_generation_and_versioned_inputs() {
    let source = source_id(1);
    let manifest = manifest(source, generation_id(11));
    let projection = ProjectionCompiler::compile_resource(
        &manifest,
        &input(source, RetentionMode::PersistentDiscoveryMetadata),
    )
    .unwrap();

    assert_eq!(projection.manifest, manifest);
    assert_eq!(
        projection.manifest.key(),
        ProjectionGenerationKey {
            source_id: source,
            generation_id: generation_id(11)
        }
    );
    assert_eq!(projection.directory.canonical_name, "canonical");
    assert_eq!(projection.directory.title.as_deref(), Some("display title"));
    assert_eq!(projection.directory.aliases, vec!["alias"]);
    assert_eq!(projection.directory.resource_ref, resource_id(100));

    let mut wrong_lens = input(source, RetentionMode::PersistentDiscoveryMetadata);
    wrong_lens.lens.lens_version = 8;
    assert!(matches!(
        ProjectionCompiler::compile_resource(&manifest, &wrong_lens),
        Err(ProjectionError::LensVersionMismatch)
    ));
    let wrong_source = input(source_id(2), RetentionMode::PersistentDiscoveryMetadata);
    assert!(matches!(
        ProjectionCompiler::compile_resource(&manifest, &wrong_source),
        Err(ProjectionError::SourceMismatch)
    ));
}

#[test]
fn compiler_projects_only_high_signal_facets_selected_by_lens() {
    let source = source_id(1);
    let input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    let projection =
        ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input).unwrap();

    assert_eq!(
        projection.structured.high_signal_facets,
        BTreeMap::from([("known".into(), FacetState::Known("yes".into()))])
    );
}

#[test]
fn compiler_preserves_four_facet_states_and_authority_conflict_evidence() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    input.lens.high_signal_facets = vec![
        "known".into(),
        "unknown".into(),
        "na".into(),
        "conflict".into(),
    ];
    input
        .typed_facets
        .insert("amount".into(), FacetState::Known(TypedValue::Integer(7)));
    input
        .typed_facets
        .insert("missing".into(), FacetState::Unknown);
    input
        .typed_facets
        .insert("inapplicable".into(), FacetState::NotApplicable);
    input
        .typed_facets
        .insert("disputed".into(), FacetState::Conflict);
    let mut assertion = Assertion::new(
        "resource-100",
        "disputed",
        TypedValue::String("a".into()),
        "source-ref",
        AssertionOrigin::Authoritative,
        "scope-a",
        at(900),
    );
    assertion.evidence_refs.push("evidence-1".into());
    input.assertions.push(assertion.clone());
    let conflict = AuthorityConflict {
        subject_ref: "resource-100".into(),
        predicate: "disputed".into(),
        authority_scope: "scope-a".into(),
        rank: 10,
        values: vec![
            TypedValue::String("a".into()),
            TypedValue::String("b".into()),
        ],
    };
    input.authority_resolutions.insert(
        "disputed".into(),
        AuthorityResolution::Conflict(conflict.clone()),
    );

    let projection =
        ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input).unwrap();
    assert_eq!(projection.structured.typed_facets, input.typed_facets);
    assert_eq!(
        projection.structured.high_signal_facets,
        input.resource.discovery_profile.high_signal_facets
    );
    assert_eq!(projection.structured.assertions, vec![assertion]);
    assert_eq!(
        projection.structured.authority_resolutions.get("disputed"),
        Some(&AuthorityResolution::Conflict(conflict))
    );
}

#[test]
fn compiler_rejects_known_typed_facet_with_same_key_authority_conflict() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    input
        .typed_facets
        .insert("amount".into(), FacetState::Known(TypedValue::Integer(7)));
    input.authority_resolutions.insert(
        "amount".into(),
        AuthorityResolution::Conflict(AuthorityConflict {
            subject_ref: "resource-100".into(),
            predicate: "amount".into(),
            authority_scope: "scope-a".into(),
            rank: 10,
            values: vec![TypedValue::Integer(7), TypedValue::Integer(8)],
        }),
    );

    let error = ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input)
        .expect_err("a known facet cannot hide an authority conflict");
    assert_eq!(
        error.to_string(),
        "typed facet `amount` contradicts authority resolution"
    );
}

#[test]
fn compiler_rejects_known_typed_facet_with_different_resolved_value() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    input.typed_facets.insert(
        "amount".into(),
        FacetState::Known(TypedValue::Decimal(DecimalValue::new(100, 2))),
    );
    input.authority_resolutions.insert(
        "amount".into(),
        AuthorityResolution::Resolved(TypedValue::Decimal(DecimalValue::new(2, 0))),
    );

    let error = ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input)
        .expect_err("a known facet must agree with the resolved value");
    assert_eq!(
        error.to_string(),
        "typed facet `amount` contradicts authority resolution"
    );
}

#[test]
fn compiler_accepts_semantically_equal_resolved_value_and_retains_assertion() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    input.typed_facets.insert(
        "amount".into(),
        FacetState::Known(TypedValue::Decimal(DecimalValue::new(100, 2))),
    );
    input.authority_resolutions.insert(
        "amount".into(),
        AuthorityResolution::Resolved(TypedValue::Decimal(DecimalValue::new(1, 0))),
    );
    let assertion = Assertion::new(
        "resource-100",
        "amount",
        TypedValue::Decimal(DecimalValue::new(1, 0)),
        "source-ref",
        AssertionOrigin::Authoritative,
        "scope-a",
        at(900),
    );
    input.assertions.push(assertion.clone());

    let projection =
        ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input).unwrap();
    assert_eq!(projection.structured.typed_facets, input.typed_facets);
    assert_eq!(projection.structured.assertions, vec![assertion]);
    assert_eq!(
        projection.structured.authority_resolutions,
        input.authority_resolutions
    );
}

fn conflict_resolution(facet: &str) -> AuthorityResolution {
    AuthorityResolution::Conflict(AuthorityConflict {
        subject_ref: "resource-100".into(),
        predicate: facet.into(),
        authority_scope: "scope-a".into(),
        rank: 10,
        values: vec![
            TypedValue::String("a".into()),
            TypedValue::String("b".into()),
        ],
    })
}

#[test]
fn compiler_enforces_typed_facet_authority_state_matrix() {
    let source = source_id(1);
    let cases = [
        (
            "resolved matching known",
            FacetState::Known(TypedValue::String("a".into())),
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            true,
        ),
        (
            "resolved different known",
            FacetState::Known(TypedValue::String("b".into())),
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved unknown",
            FacetState::Unknown,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved not applicable",
            FacetState::NotApplicable,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved conflict",
            FacetState::Conflict,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "conflict known",
            FacetState::Known(TypedValue::String("a".into())),
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict unknown",
            FacetState::Unknown,
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict not applicable",
            FacetState::NotApplicable,
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict conflict",
            FacetState::Conflict,
            conflict_resolution("facet"),
            true,
        ),
        (
            "unresolved known",
            FacetState::Known(TypedValue::String("a".into())),
            AuthorityResolution::Unresolved,
            false,
        ),
        (
            "unresolved unknown",
            FacetState::Unknown,
            AuthorityResolution::Unresolved,
            true,
        ),
        (
            "unresolved not applicable",
            FacetState::NotApplicable,
            AuthorityResolution::Unresolved,
            true,
        ),
        (
            "unresolved conflict",
            FacetState::Conflict,
            AuthorityResolution::Unresolved,
            false,
        ),
    ];

    for (name, state, resolution, accepted) in cases {
        let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
        input.typed_facets.insert("facet".into(), state);
        input
            .authority_resolutions
            .insert("facet".into(), resolution);
        let result =
            ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input);
        if accepted {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert!(
                matches!(result, Err(ProjectionError::InconsistentFacetAuthority { ref facet }) if facet == "facet"),
                "{name}: {result:?}"
            );
        }
    }
}

#[test]
fn compiler_enforces_selected_high_signal_authority_state_matrix() {
    let source = source_id(1);
    let cases = [
        (
            "resolved matching known",
            FacetState::Known("a".into()),
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            true,
        ),
        (
            "resolved different known",
            FacetState::Known("b".into()),
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved non-string known",
            FacetState::Known("7".into()),
            AuthorityResolution::Resolved(TypedValue::Integer(7)),
            false,
        ),
        (
            "resolved unknown",
            FacetState::Unknown,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved not applicable",
            FacetState::NotApplicable,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "resolved conflict",
            FacetState::Conflict,
            AuthorityResolution::Resolved(TypedValue::String("a".into())),
            false,
        ),
        (
            "conflict known",
            FacetState::Known("a".into()),
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict unknown",
            FacetState::Unknown,
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict not applicable",
            FacetState::NotApplicable,
            conflict_resolution("facet"),
            false,
        ),
        (
            "conflict conflict",
            FacetState::Conflict,
            conflict_resolution("facet"),
            true,
        ),
        (
            "unresolved known",
            FacetState::Known("a".into()),
            AuthorityResolution::Unresolved,
            false,
        ),
        (
            "unresolved unknown",
            FacetState::Unknown,
            AuthorityResolution::Unresolved,
            true,
        ),
        (
            "unresolved not applicable",
            FacetState::NotApplicable,
            AuthorityResolution::Unresolved,
            true,
        ),
        (
            "unresolved conflict",
            FacetState::Conflict,
            AuthorityResolution::Unresolved,
            false,
        ),
    ];

    for (name, state, resolution, accepted) in cases {
        let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
        input.lens.high_signal_facets = vec!["facet".into()];
        input
            .resource
            .discovery_profile
            .high_signal_facets
            .insert("facet".into(), state);
        input
            .authority_resolutions
            .insert("facet".into(), resolution);
        let result =
            ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input);
        if accepted {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert!(
                matches!(result, Err(ProjectionError::InconsistentHighSignalFacetAuthority { ref facet }) if facet == "facet"),
                "{name}: {result:?}"
            );
        }
    }
}

#[test]
fn compiler_rejects_disagreement_between_selected_high_signal_and_typed_facets() {
    let source = source_id(1);
    let cases = [
        (
            "matching strings",
            FacetState::Known("a".into()),
            FacetState::Known(TypedValue::String("a".into())),
            true,
        ),
        (
            "different strings",
            FacetState::Known("a".into()),
            FacetState::Known(TypedValue::String("b".into())),
            false,
        ),
        (
            "uncertain numeric rendering",
            FacetState::Known("7".into()),
            FacetState::Known(TypedValue::Integer(7)),
            false,
        ),
        (
            "both unknown",
            FacetState::Unknown,
            FacetState::Unknown,
            true,
        ),
        (
            "both not applicable",
            FacetState::NotApplicable,
            FacetState::NotApplicable,
            true,
        ),
        (
            "both conflict",
            FacetState::Conflict,
            FacetState::Conflict,
            true,
        ),
        (
            "unknown versus not applicable",
            FacetState::Unknown,
            FacetState::NotApplicable,
            false,
        ),
        (
            "not applicable versus unknown",
            FacetState::NotApplicable,
            FacetState::Unknown,
            false,
        ),
        (
            "conflict versus unknown",
            FacetState::Conflict,
            FacetState::Unknown,
            false,
        ),
        (
            "known versus unknown",
            FacetState::Known("a".into()),
            FacetState::Unknown,
            false,
        ),
    ];

    for (name, high_signal, typed, accepted) in cases {
        let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
        input.lens.high_signal_facets = vec!["facet".into()];
        input
            .resource
            .discovery_profile
            .high_signal_facets
            .insert("facet".into(), high_signal);
        input.typed_facets.insert("facet".into(), typed);
        let result =
            ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input);
        if accepted {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            assert!(
                matches!(result, Err(ProjectionError::InconsistentFacetProjections { ref facet }) if facet == "facet"),
                "{name}: {result:?}"
            );
        }
    }
}

#[test]
fn compiler_does_not_validate_unselected_high_signal_facets() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    input.authority_resolutions.insert(
        "unknown".into(),
        AuthorityResolution::Resolved(TypedValue::String("a".into())),
    );
    input.typed_facets.insert(
        "unknown".into(),
        FacetState::Known(TypedValue::String("a".into())),
    );

    let projection =
        ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input).unwrap();
    assert!(
        !projection
            .structured
            .high_signal_facets
            .contains_key("unknown")
    );
}

#[test]
fn relation_output_retains_n_ary_roles_qualifiers_and_provenance() {
    let source = source_id(1);
    let mut input = input(source, RetentionMode::PersistentDiscoveryMetadata);
    let relation_id = RelationId::from_uuid(Uuid::from_u128(201));
    input.resource.relation_ids.push(relation_id);
    let mut relation = TypedRelationInstance::new(
        relation_id,
        RelationNamespace::Discovery,
        "three-way",
        vec![
            RelationParticipant::new("actor", resource_id(100)),
            RelationParticipant::new("subject", resource_id(101)),
            RelationParticipant::new("witness", resource_id(102)),
        ],
    );
    relation
        .qualifiers
        .insert("condition".into(), TypedValue::String("signed".into()));
    relation.provenance = Some("source-record".into());
    relation.evidence_refs.push("evidence-2".into());
    input.relations.push(relation.clone());

    let projection =
        ProjectionCompiler::compile_resource(&manifest(source, generation_id(11)), &input).unwrap();
    assert_eq!(projection.relations, vec![relation]);
    assert_eq!(projection.access.access_scope.as_deref(), Some("team-a"));
    assert_eq!(projection.temporal.valid_from, Some(at(100)));
}

#[test]
fn deterministic_compilation_matches_full_and_incremental_input_order() {
    let source = source_id(1);
    let mut full = input(source, RetentionMode::PersistentDiscoveryMetadata);
    let first = RelationId::from_uuid(Uuid::from_u128(201));
    let second = RelationId::from_uuid(Uuid::from_u128(202));
    for id in [second, first] {
        full.resource.relation_ids.push(id);
        full.relations.push(TypedRelationInstance::new(
            id,
            RelationNamespace::Discovery,
            "group",
            vec![
                RelationParticipant::new("member", resource_id(100)),
                RelationParticipant::new("group", resource_id(102)),
            ],
        ));
    }
    let mut incremental = full.clone();
    incremental.relations.reverse();
    incremental.resource.relation_ids.reverse();
    let manifest = manifest(source, generation_id(11));
    let full_output = ProjectionCompiler::compile_resource(&manifest, &full).unwrap();
    let incremental_output = ProjectionCompiler::compile_resource(&manifest, &incremental).unwrap();
    assert_eq!(full_output, incremental_output);
}

#[test]
fn no_retention_projection_cannot_enter_persistent_store_port() {
    let source = source_id(1);
    let generation = manifest(source, generation_id(11));
    let ephemeral = ProjectionCompiler::compile_resource(
        &generation,
        &input(source, RetentionMode::NoRetention),
    )
    .unwrap();
    assert!(matches!(
        PersistableResourceProjection::try_from(ephemeral),
        Err(ProjectionError::PersistenceDenied)
    ));
    let no_retention_source = input(source, RetentionMode::NoRetention).source;
    assert!(matches!(
        PersistableGenerationManifest::try_from((generation.clone(), &no_retention_source)),
        Err(ProjectionError::PersistenceDenied)
    ));
    let durable = ProjectionCompiler::compile_resource(
        &generation,
        &input(source, RetentionMode::PersistentDiscoveryMetadata),
    )
    .unwrap();
    assert!(PersistableResourceProjection::try_from(durable).is_ok());
    let durable_source = input(source, RetentionMode::PersistentDiscoveryMetadata).source;
    assert!(PersistableGenerationManifest::try_from((generation, &durable_source)).is_ok());
}

#[test]
fn failed_or_unvalidated_generation_never_replaces_pinned_current() {
    let source = source_id(1);
    let n = manifest(source, generation_id(11));
    let next = manifest(source, generation_id(12));
    let mut publication = GenerationPublication::new(source);
    publication.begin(n.clone()).unwrap();
    publication.validate(n.key()).unwrap();
    publication.publish(n.key()).unwrap();
    let pinned = publication.pin_current().unwrap();

    publication.begin(next.clone()).unwrap();
    assert!(matches!(
        publication.publish(next.key()),
        Err(ProjectionError::GenerationNotValidated)
    ));
    publication.fail(next.key()).unwrap();
    assert!(matches!(
        publication.validate(next.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert_eq!(publication.current(), Some(n.key()));
    assert_eq!(publication.pin_current(), Some(n));
    assert_eq!(pinned.generation_id, generation_id(11));
    assert_eq!(
        publication.state(next.key()),
        Some(ProjectionPublishState::Failed)
    );

    let foreign = manifest(source_id(2), generation_id(11));
    assert!(matches!(
        publication.begin(foreign),
        Err(ProjectionError::SourceMismatch)
    ));
}

#[test]
fn published_next_generation_does_not_mutate_existing_pin() {
    let source = source_id(1);
    let n = manifest(source, generation_id(11));
    let mut next = manifest(source, generation_id(12));
    next.source_snapshot = "snapshot-43".into();
    let mut publication = GenerationPublication::new(source);
    publication.begin(n.clone()).unwrap();
    publication.validate(n.key()).unwrap();
    publication.publish(n.key()).unwrap();
    let pinned_n = publication.pin_current().unwrap();
    publication.begin(next.clone()).unwrap();
    publication.validate(next.key()).unwrap();
    publication.publish(next.key()).unwrap();

    assert_eq!(publication.current(), Some(next.key()));
    assert_eq!(publication.pin_current(), Some(next));
    assert_eq!(pinned_n, n);
    assert_eq!(
        publication.state(n.key()),
        Some(ProjectionPublishState::Retired)
    );
}

#[test]
fn previously_published_generation_cannot_fail_after_a_new_publish() {
    let source = source_id(1);
    let n = manifest(source, generation_id(11));
    let next = manifest(source, generation_id(12));
    let mut publication = GenerationPublication::new(source);
    publication.begin(n.clone()).unwrap();
    publication.validate(n.key()).unwrap();
    publication.publish(n.key()).unwrap();
    let pinned_n = publication.pin_current().unwrap();
    publication.begin(next.clone()).unwrap();
    publication.validate(next.key()).unwrap();
    publication.publish(next.key()).unwrap();

    assert!(matches!(
        publication.fail(n.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert_eq!(pinned_n, n);
    assert_eq!(
        publication.state(n.key()),
        Some(ProjectionPublishState::Retired)
    );
    assert_eq!(publication.current(), Some(next.key()));
    assert_eq!(publication.pin_current(), Some(next));
}

#[test]
fn previously_published_generation_cannot_be_republished() {
    let source = source_id(1);
    let n = manifest(source, generation_id(11));
    let next = manifest(source, generation_id(12));
    let mut publication = GenerationPublication::new(source);
    publication.begin(n.clone()).unwrap();
    publication.validate(n.key()).unwrap();
    publication.publish(n.key()).unwrap();
    publication.begin(next.clone()).unwrap();
    publication.validate(next.key()).unwrap();
    publication.publish(next.key()).unwrap();

    assert!(matches!(
        publication.publish(n.key()),
        Err(ProjectionError::GenerationNotValidated)
    ));
    assert_eq!(publication.current(), Some(next.key()));
    assert_eq!(publication.pin_current(), Some(next));
}

#[test]
fn generation_publication_rejects_terminal_and_repeat_transitions() {
    let source = source_id(1);
    let failed = manifest(source, generation_id(21));
    let n = manifest(source, generation_id(22));
    let next = manifest(source, generation_id(23));
    let mut publication = GenerationPublication::new(source);

    publication.begin(failed.clone()).unwrap();
    assert_eq!(
        publication.state(failed.key()),
        Some(ProjectionPublishState::Building)
    );
    publication.validate(failed.key()).unwrap();
    assert_eq!(
        publication.state(failed.key()),
        Some(ProjectionPublishState::Validated)
    );
    assert!(matches!(
        publication.validate(failed.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    publication.fail(failed.key()).unwrap();
    assert_eq!(
        publication.state(failed.key()),
        Some(ProjectionPublishState::Failed)
    );
    assert!(matches!(
        publication.validate(failed.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.fail(failed.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.publish(failed.key()),
        Err(ProjectionError::GenerationNotValidated)
    ));
    assert!(matches!(
        publication.begin(failed),
        Err(ProjectionError::DuplicateGeneration)
    ));

    publication.begin(n.clone()).unwrap();
    publication.validate(n.key()).unwrap();
    publication.publish(n.key()).unwrap();
    assert_eq!(
        publication.state(n.key()),
        Some(ProjectionPublishState::Current)
    );
    assert!(matches!(
        publication.validate(n.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.fail(n.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.publish(n.key()),
        Err(ProjectionError::GenerationNotValidated)
    ));

    publication.begin(next.clone()).unwrap();
    publication.validate(next.key()).unwrap();
    publication.publish(next.key()).unwrap();
    assert_eq!(
        publication.state(n.key()),
        Some(ProjectionPublishState::Retired)
    );
    assert!(matches!(
        publication.validate(n.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.fail(n.key()),
        Err(ProjectionError::InvalidGenerationTransition)
    ));
    assert!(matches!(
        publication.publish(n.key()),
        Err(ProjectionError::GenerationNotValidated)
    ));
    assert!(matches!(
        publication.begin(n),
        Err(ProjectionError::DuplicateGeneration)
    ));
    assert_eq!(publication.current(), Some(next.key()));
}
