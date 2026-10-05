//! P4-09: metadata-only durable field proof for remote projections.

#[path = "support/remote.rs"]
mod support;

use std::collections::BTreeMap;

use search_application::ports::CurrentSourcePolicy;
use search_application::projection::{
    ProjectionError, RemoteFieldKind, RemoteFieldProofs, VerifiedPersistentProjection,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::id::{ProjectionGenerationId, RelationId, ResourceId};
use search_core::materialization::ProviderContentPermission;
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::profile::FacetState;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use search_core::temporal::TemporalDiscoveryProfile;
use support::*;
use time::OffsetDateTime;
use uuid::Uuid;

fn projection(
    retention: RetentionMode,
    facets: &[(&str, TypedValue)],
) -> CompiledResourceProjection {
    let registration = registration(retention);
    let id = ResourceId::from_uuid(Uuid::from_u128(77));
    CompiledResourceProjection {
        manifest: ProjectionGenerationManifest {
            source_id: registration.source_id(),
            generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
            projection_schema_version: "remote-evaluation-v1".into(),
            lens_version: 1,
            semantic_registry_version: "remote".into(),
            analyzer_version: None,
            embedding_model_version: None,
            graph_schema_version: None,
            source_snapshot: "remote-snapshot".into(),
            resource_count: 1,
            relation_count: Some(0),
            coverage: Coverage::QueryResult,
            digest: format!("sha256:{}", "0".repeat(64)),
            built_at: OffsetDateTime::now_utc(),
        },
        retention_mode: retention,
        directory: DirectoryProjection {
            resource_ref: id,
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: "規程".into(),
            title: Some("規程".into()),
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: id,
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets: facets
                .iter()
                .map(|(name, value)| ((*name).to_owned(), FacetState::Known(value.clone())))
                .collect(),
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: id,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: id,
            access_scope: None,
            source_access_model: Some("per-item".into()),
        },
        relations: vec![],
    }
}

fn policy(retention: RetentionMode) -> CurrentSourcePolicy {
    CurrentSourcePolicy {
        resource_kind: ResourceKind::Knowledge,
        provider_permission: ProviderContentPermission::Metadata,
        retention_mode: retention,
        probe_allowed: false,
    }
}

fn proofs() -> RemoteFieldProofs {
    RemoteFieldProofs::new(vec![
        ("department".into(), RemoteFieldKind::Text),
        ("revision".into(), RemoteFieldKind::Integer),
    ])
    .unwrap()
}

fn text(value: &str) -> TypedValue {
    TypedValue::String(value.into())
}

#[test]
fn metadata_only_rejects_body_fragment_content_assertion_evidence_and_relation() {
    let mode = RetentionMode::PersistentDiscoveryMetadata;
    let registration = registration(mode);
    for field in ["body", "fragment", "content", "snippet"] {
        let refused = VerifiedPersistentProjection::try_from_remote(
            projection(mode, &[(field, text("本文"))]),
            &registration,
            &policy(mode),
            &proofs(),
        );
        assert!(
            matches!(refused, Err(ProjectionError::FieldNotPermitted { .. })),
            "{field}"
        );
    }
    // A content-bearing assertion with evidence refs.
    let mut with_assertion = projection(mode, &[("department", text("総務"))]);
    let mut assertion = Assertion::new(
        "doc",
        "catalog.title",
        text("規程"),
        "remote",
        AssertionOrigin::Observed,
        "catalog",
        OffsetDateTime::now_utc(),
    );
    assertion.evidence_refs = vec!["ev-1".into()];
    with_assertion.structured.assertions.push(assertion);
    assert_eq!(
        VerifiedPersistentProjection::try_from_remote(
            with_assertion,
            &registration,
            &policy(mode),
            &proofs()
        ),
        Err(ProjectionError::ContentNotPermitted)
    );
    // A relation.
    let mut with_relation = projection(mode, &[]);
    let id = with_relation.directory.resource_ref;
    with_relation.relations.push(TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(5)),
        RelationNamespace::Discovery,
        "cites",
        vec![
            RelationParticipant::new("source", id),
            RelationParticipant::new("target", ResourceId::from_uuid(Uuid::from_u128(6))),
        ],
    ));
    assert_eq!(
        VerifiedPersistentProjection::try_from_remote(
            with_relation,
            &registration,
            &policy(mode),
            &proofs()
        ),
        Err(ProjectionError::ContentNotPermitted)
    );
}

#[test]
fn permitted_identity_title_facet_provenance_persists() {
    let mode = RetentionMode::PersistentDiscoveryMetadata;
    let persisted = VerifiedPersistentProjection::try_from_remote(
        projection(
            mode,
            &[
                ("department", text("総務")),
                ("revision", TypedValue::Integer(3)),
            ],
        ),
        &registration(mode),
        &policy(mode),
        &proofs(),
    )
    .unwrap();
    let inner = persisted.projection();
    assert_eq!(inner.directory.title.as_deref(), Some("規程"));
    assert_eq!(inner.structured.typed_facets.len(), 2);
    // A proven name with the wrong value kind is refused.
    assert!(matches!(
        VerifiedPersistentProjection::try_from_remote(
            projection(mode, &[("revision", text("three"))]),
            &registration(mode),
            &policy(mode),
            &proofs(),
        ),
        Err(ProjectionError::FieldNotPermitted { .. })
    ));
}

#[test]
fn credential_and_raw_provider_response_never_persist() {
    for mode in [
        RetentionMode::PersistentResource,
        RetentionMode::PersistentDiscoveryMetadata,
    ] {
        for field in [
            "authorization",
            "token",
            "raw_response",
            "provider_response",
            "credential",
        ] {
            assert!(matches!(
                VerifiedPersistentProjection::try_from_remote(
                    projection(mode, &[(field, text("secret-like"))]),
                    &registration(mode),
                    &policy(mode),
                    &proofs(),
                ),
                Err(ProjectionError::FieldNotPermitted { .. })
            ));
        }
        // An unproven field is refused even when it looks harmless.
        assert!(
            VerifiedPersistentProjection::try_from_remote(
                projection(mode, &[("unlisted", text("x"))]),
                &registration(mode),
                &policy(mode),
                &proofs(),
            )
            .is_err()
        );
    }
    // The proofs themselves cannot admit a reserved name.
    assert!(RemoteFieldProofs::new(vec![("raw_response".into(), RemoteFieldKind::Text)]).is_err());
}

#[test]
fn session_cache_no_retention_modes_still_rejected() {
    for mode in [
        RetentionMode::SessionOnly,
        RetentionMode::CacheWithExpiry,
        RetentionMode::NoRetention,
    ] {
        assert_eq!(
            VerifiedPersistentProjection::try_from_remote(
                projection(mode, &[("department", text("総務"))]),
                &registration(mode),
                &policy(mode),
                &proofs(),
            ),
            Err(ProjectionError::PersistenceDenied)
        );
    }
    // A current policy that tightened retention refuses the old projection.
    let mode = RetentionMode::PersistentDiscoveryMetadata;
    assert_eq!(
        VerifiedPersistentProjection::try_from_remote(
            projection(mode, &[("department", text("総務"))]),
            &registration(mode),
            &policy(RetentionMode::SessionOnly),
            &proofs(),
        ),
        Err(ProjectionError::PersistenceDenied)
    );
}
