//! P3-G05: Document mapping commitment and current access of durable Graph
//! nodes, on a real PostgreSQL Document Source. Stored rows are checked
//! against their owner, and every decision re-reads current Document state.

#[path = "support/document_discovery.rs"]
mod discovery_support;
#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod versioning_support;

use std::sync::Arc;

use document_application::{
    AccessPolicyService, BootstrapRootPolicy, DocumentAccessCheckService, InvocationKind,
    ManagementCommand, ManagementOperationId, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentId, FolderId, PolicyGrant, PolicyMode, PolicySubject, PolicySubjectKind,
    PolicyTarget,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use search_application::graph_generation::{GraphResourceRecord, GraphSourceMapping};
use search_application::ports::AccessDecision;
use search_core::id::{ProjectionGenerationId, ResourceId, ResourceVersionId, SourceId};
use search_core::projection::{ProjectionGenerationKey, TemporalProjection};
use search_core::resource::ResourceKind;
use search_graph::{PostgresGraphStore, canonical_mapping_digest};
use search_source_document::{
    DocumentCurrentAccessAdapter, DocumentGenerationAccess, document_resource_id,
    folder_resource_id, validate_document_graph_mapping,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const SNAPSHOT: &str = "document-snapshot-1";

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        versioning_support::actor(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

fn grant(actions: impl IntoIterator<Item = Action>) -> PolicyGrant {
    PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
        actions,
    )
    .unwrap()
}

fn record(id: ResourceId, kind: ResourceKind, mapping: GraphSourceMapping) -> GraphResourceRecord {
    GraphResourceRecord {
        resource_ref: id,
        kind,
        resource_version_ref: match mapping {
            GraphSourceMapping::Version { version_id, .. } => {
                Some(ResourceVersionId::from_uuid(version_id))
            }
            _ => None,
        },
        temporal: TemporalProjection {
            resource_ref: id,
            valid_from: None,
            valid_to: None,
            profile: Default::default(),
        },
        mapping,
        attached_relations: vec![],
    }
}

/// The Document, folder-placement and Version nodes of one Document.
fn nodes(source: SourceId, document: DocumentId, version: Uuid) -> [GraphResourceRecord; 3] {
    let folder = FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID);
    [
        record(
            document_resource_id(source, document),
            ResourceKind::Document,
            GraphSourceMapping::Document {
                document_id: document.as_uuid(),
            },
        ),
        record(
            folder_resource_id(source, document, folder),
            ResourceKind::FolderPlacement,
            GraphSourceMapping::FolderPlacement {
                document_id: document.as_uuid(),
                folder_id: folder.as_uuid(),
            },
        ),
        record(
            ResourceId::from_uuid(version),
            ResourceKind::Knowledge,
            GraphSourceMapping::Version {
                document_id: document.as_uuid(),
                version_id: version,
            },
        ),
    ]
}

struct Setup {
    fixture: versioning_support::Fixture,
    source: SourceId,
    repository: Arc<PostgresDocumentRepository>,
    access: DocumentCurrentAccessAdapter,
}

impl Setup {
    async fn start() -> Self {
        let fixture = versioning_support::fixture().await;
        let source = SourceId::from_uuid(Uuid::now_v7());
        let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            fixture.pool.clone(),
            versioning_support::actor(),
        ));
        repository
            .initialize_root_policy(&actor(), vec![grant([Action::Read, Action::Administer])])
            .await
            .unwrap();
        let access = DocumentCurrentAccessAdapter::new(
            source,
            fixture.pool.clone(),
            DocumentAccessCheckService::new(repository.clone()),
            actor(),
            discovery_support::ACCESS_CONTEXT.into(),
        );
        Self {
            fixture,
            source,
            repository,
            access,
        }
    }

    fn key(&self, source: SourceId) -> ProjectionGenerationKey {
        ProjectionGenerationKey {
            source_id: source,
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(1)),
        }
    }

    fn gate(&self) -> DocumentGenerationAccess<'_> {
        DocumentGenerationAccess::new(
            PostgresGraphStore::new(self.fixture.pool.clone()),
            &self.access,
            discovery_support::ACCESS_CONTEXT,
        )
    }

    async fn decisions(
        &self,
        key: ProjectionGenerationKey,
        records: &[GraphResourceRecord],
    ) -> Vec<AccessDecision> {
        let gate = self.gate();
        let mut decisions = Vec::new();
        for record in records {
            decisions.push(
                gate.evaluate_stored(key, record, discovery_support::ACCESS_CONTEXT)
                    .await
                    .unwrap(),
            );
        }
        decisions
    }

    /// A second readable Document in the same Source.
    async fn other_document(&self) -> DocumentId {
        let document = Uuid::now_v7();
        let version = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
             VALUES ($1,$2,NULL,1,'{}',to_timestamp(0))",
        )
        .bind(document)
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&self.fixture.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
             title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
             VALUES ($1,$2,1,'PUBLISHED','Other',to_timestamp(0),'test-idp','editor','{}',to_timestamp(0))",
        )
        .bind(version)
        .bind(document)
        .execute(&self.fixture.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
            .bind(version)
            .bind(document)
            .execute(&self.fixture.pool)
            .await
            .unwrap();
        DocumentId::from_uuid(document)
    }
}

use AccessDecision::{Allowed, Denied};

#[tokio::test]
async fn cross_source_or_spoofed_owner_is_denied() {
    let setup = Setup::start().await;
    let document = setup.fixture.document_id;
    let base = setup.fixture.base_id.as_uuid();
    let own = nodes(setup.source, document, base);
    let key = setup.key(setup.source);
    assert_eq!(setup.decisions(key, &own).await, vec![Allowed; 3]);
    let digest = canonical_mapping_digest(setup.source, SNAPSHOT, &own).unwrap();
    validate_document_graph_mapping(setup.source, SNAPSHOT, &own, &digest).unwrap();

    // cross_source_access_cannot_borrow_owner: another Source's key, or nodes
    // whose ResourceIds were derived for another Source, are never allowed.
    let other_source = SourceId::from_uuid(Uuid::now_v7());
    assert_eq!(
        setup.decisions(setup.key(other_source), &own).await,
        vec![Denied; 3]
    );
    let borrowed = nodes(other_source, document, base);
    assert_eq!(setup.decisions(key, &borrowed[..2]).await, vec![Denied; 2]);

    // folder_placement_spoof_or_swapped_owner_is_denied: a readable owner
    // swapped into a node whose ResourceId belongs to another Document.
    let other = setup.other_document().await;
    let mut swapped = own[1].clone();
    swapped.mapping = GraphSourceMapping::FolderPlacement {
        document_id: other.as_uuid(),
        folder_id: SYSTEM_ROOT_FOLDER_ID,
    };
    let mut kind_swap = own[0].clone();
    kind_swap.kind = ResourceKind::FolderPlacement;
    let mut wrong_version_owner = own[2].clone();
    wrong_version_owner.mapping = GraphSourceMapping::Version {
        document_id: other.as_uuid(),
        version_id: base,
    };
    let spoofed = [swapped, kind_swap, wrong_version_owner];
    assert_eq!(setup.decisions(key, &spoofed).await, vec![Denied; 3]);
    // Rows changed after the commitment never validate against it. The two
    // structural spoofs fail even under a digest recomputed over them; a
    // Version owner is not recomputable and relies on the commitment plus the
    // direct current-Version decision above.
    for (position, node) in [1, 0, 2].into_iter().zip(&spoofed) {
        let mut records = own.clone();
        records[position] = node.clone();
        assert!(
            validate_document_graph_mapping(setup.source, SNAPSHOT, &records, &digest).is_err()
        );
        let recomputed =
            canonical_mapping_digest(setup.source, SNAPSHOT, &records).unwrap_or_default();
        assert_eq!(
            validate_document_graph_mapping(setup.source, SNAPSHOT, &records, &recomputed).is_err(),
            position != 2
        );
    }
    // A changed commitment is refused even for well-formed rows.
    assert!(
        validate_document_graph_mapping(setup.source, "another-snapshot", &own, &digest).is_err()
    );
}

#[tokio::test]
async fn version_uses_direct_current_access() {
    let setup = Setup::start().await;
    let document = setup.fixture.document_id;
    let base = setup.fixture.base_id.as_uuid();
    let key = setup.key(setup.source);
    let before = nodes(setup.source, document, base);
    assert_eq!(setup.decisions(key, &before).await, vec![Allowed; 3]);

    // A new current Version: the Document stays readable, the old Version
    // node is no longer current and the new one is.
    let next = versioning_support::install_new_current(&setup.fixture, "Next", 7).await;
    assert_eq!(
        setup.decisions(key, &before).await,
        vec![Allowed, Allowed, Denied]
    );
    let after = nodes(setup.source, document, next.as_uuid());
    assert_eq!(setup.decisions(key, &after[2..]).await, vec![Allowed]);
}

#[tokio::test]
async fn revoke_or_publication_end_after_stage_hides_path() {
    let setup = Setup::start().await;
    let document = setup.fixture.document_id;
    let base = setup.fixture.base_id.as_uuid();
    let key = setup.key(setup.source);
    let staged = nodes(setup.source, document, base);
    let other = setup.other_document().await;
    let other_version: Uuid =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(other.as_uuid())
            .fetch_one(&setup.fixture.pool)
            .await
            .unwrap();
    let other_nodes = nodes(setup.source, other, other_version);
    assert_eq!(setup.decisions(key, &staged).await, vec![Allowed; 3]);
    assert_eq!(setup.decisions(key, &other_nodes).await, vec![Allowed; 3]);

    // Read revoked after the generation was staged.
    AccessPolicyService::new(setup.repository.clone())
        .set_access_policy(
            &actor(),
            ManagementCommand::SetAccessPolicy {
                operation_id: ManagementOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                target: PolicyTarget::Document(document),
                expected_policy_revision: 0,
                mode: PolicyMode::Explicit(vec![grant([Action::Administer])]),
                reason: "revoke current read".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(setup.decisions(key, &staged).await, vec![Denied; 3]);

    // Publication of the other Document ends after staging.
    sqlx::query(
        "INSERT INTO document_publication_end_operations \
         (operation_id, document_id, command_digest, expected_document_revision, \
          expected_current_version_id, actor_identity_provider, actor_principal_id, reason, \
          former_current_version_id, resulting_document_revision, ended_at) \
         VALUES ($1, $2, $3, 1, $4, 'test-idp', 'test-user', 'test end', $4, 2, now())",
    )
    .bind(Uuid::now_v7())
    .bind(other.as_uuid())
    .bind(vec![1_u8; 32])
    .bind(other_version)
    .execute(&setup.fixture.pool)
    .await
    .unwrap();
    assert_eq!(setup.decisions(key, &other_nodes).await, vec![Denied; 3]);
}
