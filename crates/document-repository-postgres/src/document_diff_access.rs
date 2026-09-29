use document_application::{
    RepositoryError, VerifiedActorContext,
    document_diff::{DiffPairSnapshot, DiffRequest, DiffResult},
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, lock_access_state},
    document_diff_snapshot::capture_version,
    error::{map_commit_error, map_statement_error},
};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";
const AUDIT_EVENT: &str = "document.diff.result_access_granted";

impl PostgresDocumentRepository {
    pub(crate) async fn finalize_document_diff_result(
        &self,
        actor: &VerifiedActorContext,
        pair: &DiffPairSnapshot,
        result: &DiffResult,
        cache_hit: bool,
        correlation_id: Option<&str>,
    ) -> Result<Uuid, RepositoryError> {
        actor
            .ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        pair.validate().map_err(|_| RepositoryError::BusinessRule)?;
        result
            .validate()
            .map_err(|_| RepositoryError::BusinessRule)?;
        if result.document_id != pair.document_id
            || result.base_version_id != pair.base.version_id
            || result.target_version_id != pair.target.version_id
            || result.base_snapshot_digest != pair.base.snapshot_digest()
            || result.target_snapshot_digest != pair.target.snapshot_digest()
            || result.canonical_bytes().len() > 16 * 1024 * 1024
            || correlation_id.is_some_and(|value| value.len() > 128)
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        let mut tx = self.pool.begin().await.map_err(map_statement_error)?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        // Lock the Document row against version switches, withdrawal and T10 until audit commit.
        sqlx::query("SELECT revision FROM documents WHERE document_id = $1 FOR SHARE")
            .bind(pair.document_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::DocumentNotFound)?;
        let request = DiffRequest {
            document_id: pair.document_id,
            base_version_id: pair.base.version_id,
            target_version_id: pair.target.version_id,
            profile: result.profile,
        };
        let latest_base = capture_version(&mut tx, actor, &request, pair.base.version_id).await?;
        let latest_target =
            capture_version(&mut tx, actor, &request, pair.target.version_id).await?;
        let working: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_versions \
             WHERE document_id = $1 AND document_version_id = ANY($2::uuid[]) \
               AND lifecycle_state = 'WORKING')",
        )
        .bind(pair.document_id.as_uuid())
        .bind(vec![
            pair.base.version_id.as_uuid(),
            pair.target.version_id.as_uuid(),
        ])
        .fetch_one(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        if latest_base.snapshot_digest() != pair.base.snapshot_digest()
            || latest_target.snapshot_digest() != pair.target.snapshot_digest()
            || (working
                && (latest_base.document_revision != pair.base.document_revision
                    || latest_target.document_revision != pair.target.document_revision))
        {
            return Err(RepositoryError::StaleComparisonInput);
        }
        actor
            .ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        let audit_event_id = Uuid::now_v7();
        let result_digest = result.canonical_digest();
        let payload = json!({
            "base_version_id": pair.base.version_id.as_uuid(),
            "target_version_id": pair.target.version_id.as_uuid(),
            "base_snapshot_digest": result.base_snapshot_digest,
            "target_snapshot_digest": result.target_snapshot_digest,
            "comparison_profile": result.profile.as_str(),
            "resource_profile": result.resource_profile.as_str(),
            "result_digest": result_digest,
            "verdict": result.verdict,
            "coverage": result.coverage,
            "cache_hit": cache_hit,
        });
        sqlx::query(
            "INSERT INTO audit_outbox_events \
             (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
              resource_type,resource_id,resource_version_id,result,trace_id,data,occurred_at) \
             VALUES ($1,$2,$3,$4,$5,$6,'Document',$7,$8,'success',$9,$10,now())",
        )
        .bind(audit_event_id)
        .bind(AUDIT_EVENT)
        .bind(AUDIT_SOURCE)
        .bind(format!("document/{}/diff", pair.document_id.as_uuid()))
        .bind(actor.principal().identity_provider())
        .bind(actor.principal().principal_id())
        .bind(pair.document_id.as_uuid())
        .bind(pair.target.version_id.as_uuid())
        .bind(correlation_id)
        .bind(payload)
        .execute(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        actor
            .ensure_current()
            .map_err(|_| RepositoryError::Forbidden)?;
        tx.commit().await.map_err(map_commit_error)?;
        Ok(audit_event_id)
    }
}
