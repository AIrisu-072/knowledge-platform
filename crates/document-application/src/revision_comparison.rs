use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use document_diff_core::DiffProfileVersion;
use document_domain::{DocumentId, DocumentVersionId};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::document_diff::{
    AuthorizedDiff, DiffCache, DiffExecutor, DiffInspectionEvidence, DiffRequest,
    DocumentDiffRepository, DocumentDiffService,
};
use crate::{
    ApplicationError, DocumentRevisionDetail, DocumentRevisionDetailQuery,
    DocumentRevisionReadRepository, DocumentRevisionReadService, FileStorage,
    RevisionComparisonAuditRequest, VerifiedActorContext, VersionFileAccessRepository,
};

const METADATA_SNAPSHOT_DOMAIN: &[u8] = b"document-revision-metadata-snapshot-v0\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataComparisonStatus {
    Same,
    Different,
    UnavailableLegacy,
}

impl MetadataComparisonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Different => "different",
            Self::UnavailableLegacy => "unavailableLegacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataChange {
    pub json_pointer: String,
    pub base_value: Option<Value>,
    pub target_value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataComparison {
    pub status: MetadataComparisonStatus,
    pub changes: Vec<MetadataChange>,
    pub base_snapshot_digest: Option<[u8; 32]>,
    pub target_snapshot_digest: Option<[u8; 32]>,
}

pub fn compare_revision_metadata(
    base_status: &str,
    base_snapshot: Option<&Value>,
    target_status: &str,
    target_snapshot: Option<&Value>,
) -> Result<MetadataComparison, ApplicationError> {
    let base_snapshot = available_snapshot(base_status, base_snapshot)?;
    let target_snapshot = available_snapshot(target_status, target_snapshot)?;
    let base_snapshot_digest = base_snapshot.map(metadata_digest);
    let target_snapshot_digest = target_snapshot.map(metadata_digest);
    let (Some(base_snapshot), Some(target_snapshot)) = (base_snapshot, target_snapshot) else {
        return Ok(MetadataComparison {
            status: MetadataComparisonStatus::UnavailableLegacy,
            changes: Vec::new(),
            base_snapshot_digest,
            target_snapshot_digest,
        });
    };
    let mut changes = Vec::new();
    compare_json_values("", Some(base_snapshot), Some(target_snapshot), &mut changes);
    let status = if changes.is_empty() {
        MetadataComparisonStatus::Same
    } else {
        MetadataComparisonStatus::Different
    };
    Ok(MetadataComparison {
        status,
        changes,
        base_snapshot_digest,
        target_snapshot_digest,
    })
}

fn available_snapshot<'a>(
    status: &str,
    snapshot: Option<&'a Value>,
) -> Result<Option<&'a Value>, ApplicationError> {
    match (status, snapshot) {
        ("complete", Some(snapshot)) => Ok(Some(snapshot)),
        ("unavailable_legacy", None) => Ok(None),
        _ => Err(ApplicationError::IntegrityViolation),
    }
}

fn compare_json_values(
    path: &str,
    base: Option<&Value>,
    target: Option<&Value>,
    changes: &mut Vec<MetadataChange>,
) {
    if base == target {
        return;
    }
    match (base, target) {
        (Some(Value::Object(base_object)), Some(Value::Object(target_object))) => {
            let keys = base_object
                .keys()
                .chain(target_object.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                let child_path = format!("{path}/{escaped}");
                compare_json_values(
                    &child_path,
                    base_object.get(&key),
                    target_object.get(&key),
                    changes,
                );
            }
        }
        _ => changes.push(MetadataChange {
            json_pointer: path.to_owned(),
            base_value: base.cloned(),
            target_value: target.cloned(),
        }),
    }
}

fn metadata_digest(snapshot: &Value) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(METADATA_SNAPSHOT_DOMAIN);
    digest.update(serde_json::to_vec(snapshot).expect("metadata snapshot is JSON"));
    digest.finalize().into()
}

pub enum RevisionContentComparison {
    SameAuthoritativeVersion,
    DifferentAuthoritativeVersions(Box<AuthorizedDiff>),
}

pub struct RevisionComparison {
    pub base: DocumentRevisionDetail,
    pub target: DocumentRevisionDetail,
    pub content: RevisionContentComparison,
    pub metadata: MetadataComparison,
    pub audit_event_id: Uuid,
    pub content_audit_event_id: Option<Uuid>,
}

pub trait RevisionContentComparator: Send + Sync {
    fn compare<'a>(
        &'a self,
        ctx: &'a VerifiedActorContext,
        request: DiffRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizedDiff, ApplicationError>> + Send + 'a>>;
}

impl<R, F, E, I> RevisionContentComparator for DocumentDiffService<R, F, E, I>
where
    R: DocumentDiffRepository + VersionFileAccessRepository + DiffCache + Send + Sync + 'static,
    F: FileStorage + Send + Sync + 'static,
    E: DiffExecutor + Send + Sync + 'static,
    I: DiffInspectionEvidence + Send + Sync + 'static,
{
    fn compare<'a>(
        &'a self,
        ctx: &'a VerifiedActorContext,
        request: DiffRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AuthorizedDiff, ApplicationError>> + Send + 'a>> {
        Box::pin(DocumentDiffService::compare(self, ctx, request))
    }
}

pub struct RevisionComparisonService<R, C> {
    repository: Arc<R>,
    content_comparator: Arc<C>,
}

impl<R, C> RevisionComparisonService<R, C>
where
    R: DocumentRevisionReadRepository,
    C: RevisionContentComparator,
{
    pub fn new(repository: Arc<R>, content_comparator: Arc<C>) -> Self {
        Self {
            repository,
            content_comparator,
        }
    }

    pub async fn compare(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
        base_revision_id: Uuid,
        target_revision_id: Uuid,
    ) -> Result<RevisionComparison, ApplicationError> {
        ctx.ensure_current()?;
        if base_revision_id == target_revision_id {
            return Err(ApplicationError::Validation(
                "base and target revisions must differ".into(),
            ));
        }
        let service = DocumentRevisionReadService::new(self.repository.clone());
        let base = service
            .get_document_revision(
                ctx,
                DocumentRevisionDetailQuery {
                    document_id,
                    revision_id: base_revision_id,
                },
            )
            .await?;
        let target = service
            .get_document_revision(
                ctx,
                DocumentRevisionDetailQuery {
                    document_id,
                    revision_id: target_revision_id,
                },
            )
            .await?;
        if base.summary.revision_id != base_revision_id
            || target.summary.revision_id != target_revision_id
        {
            return Err(ApplicationError::IntegrityViolation);
        }
        let metadata = compare_revision_metadata(
            &base.summary.metadata_snapshot_status,
            base.metadata_snapshot.as_ref(),
            &target.summary.metadata_snapshot_status,
            target.metadata_snapshot.as_ref(),
        )?;
        let content = if base.summary.document_version_id == target.summary.document_version_id {
            RevisionContentComparison::SameAuthoritativeVersion
        } else {
            let authorized = self
                .content_comparator
                .compare(
                    ctx,
                    DiffRequest {
                        document_id,
                        base_version_id: base.summary.document_version_id,
                        target_version_id: target.summary.document_version_id,
                        profile: DiffProfileVersion::V0,
                    },
                )
                .await?;
            RevisionContentComparison::DifferentAuthoritativeVersions(Box::new(authorized))
        };
        let (content_comparison_status, content_result_digest, content_audit_event_id) =
            match &content {
                RevisionContentComparison::SameAuthoritativeVersion => {
                    ("sameAuthoritativeVersion", None, None)
                }
                RevisionContentComparison::DifferentAuthoritativeVersions(authorized) => (
                    "differentAuthoritativeVersions",
                    Some(authorized.result_digest),
                    Some(authorized.audit_event_id),
                ),
            };
        let audit_event_id = self
            .repository
            .authorize_and_audit_revision_comparison(
                ctx,
                RevisionComparisonAuditRequest {
                    document_id,
                    base_revision_id,
                    target_revision_id,
                    content_comparison_status: content_comparison_status.into(),
                    content_result_digest,
                    content_audit_event_id,
                    metadata_comparison_status: metadata.status.as_str().into(),
                    base_metadata_snapshot_digest: metadata.base_snapshot_digest,
                    target_metadata_snapshot_digest: metadata.target_snapshot_digest,
                },
            )
            .await?;
        Ok(RevisionComparison {
            base,
            target,
            content,
            metadata,
            audit_event_id,
            content_audit_event_id,
        })
    }
}

pub fn document_version_pair(
    comparison: &RevisionComparison,
) -> (DocumentVersionId, DocumentVersionId) {
    (
        comparison.base.summary.document_version_id,
        comparison.target.summary.document_version_id,
    )
}
