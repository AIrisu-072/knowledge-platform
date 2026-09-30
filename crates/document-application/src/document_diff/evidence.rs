use std::future::Future;

use document_domain::FileId;
use document_semantic_inspection_core::InspectionProfileVersion;

use crate::{
    ApplicationError, Clock, EnsureSemanticInspection, FileStorage, SemanticInspectionExecutor,
    SemanticInspectionRecord, SemanticInspectionRepository, VerifiedActorContext,
};

use super::{DiffPairSnapshot, DiffRequest, DocumentDiffRepository, SnapshotItem};

pub trait DiffInspectionEvidence: Send + Sync {
    fn ensure(
        &self,
        file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> impl Future<Output = Result<SemanticInspectionRecord, ApplicationError>> + Send;
}

impl<R, F, E, C> DiffInspectionEvidence for EnsureSemanticInspection<R, F, E, C>
where
    R: SemanticInspectionRepository,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    C: Clock,
{
    async fn ensure(
        &self,
        file_id: FileId,
        profile: InspectionProfileVersion,
    ) -> Result<SemanticInspectionRecord, ApplicationError> {
        EnsureSemanticInspection::ensure(self, file_id, profile).await
    }
}

/// Re-capture after DSI writes; never attach new evidence to a stale source binding.
pub async fn capture_pair_with_evidence<R: DocumentDiffRepository, E: DiffInspectionEvidence>(
    repository: &R,
    evidence: &E,
    actor: &VerifiedActorContext,
    request: DiffRequest,
) -> Result<DiffPairSnapshot, ApplicationError> {
    actor.ensure_current()?;
    let first = repository.capture_pair(actor, request).await?;
    let missing: Vec<SnapshotItem> = first
        .base
        .items
        .iter()
        .chain(first.target.items.iter())
        .filter(|item| item.inspection_binding_digest.is_none())
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(first);
    }
    for item in &missing {
        match evidence.ensure(item.file_id, item.inspection_profile).await {
            Ok(record) => {
                if record.response().observed_raw_content_hash != item.raw_sha256
                    || record.response().observed_size_bytes != item.size_bytes
                {
                    return Err(ApplicationError::IntegrityViolation);
                }
            }
            Err(ApplicationError::InspectionFailed(_)) => {
                // The affected item remains explicitly unverified.
            }
            Err(error) => return Err(error),
        }
    }
    let final_pair = repository.capture_pair(actor, request).await?;
    for (before, after) in [
        (&first.base, &final_pair.base),
        (&first.target, &final_pair.target),
    ] {
        if before.document_revision != after.document_revision
            || before.source_binding_digest() != after.source_binding_digest()
        {
            return Err(ApplicationError::StaleComparisonInput);
        }
    }
    Ok(final_pair)
}
