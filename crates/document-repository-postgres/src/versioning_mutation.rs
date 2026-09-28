use document_application::{
    AUDIT_DOCUMENT_VERSION_CREATED, AUDIT_DOCUMENT_VERSION_REBASED, AUDIT_DOCUMENT_VERSION_UPDATED,
    DOCUMENT_VERSION_CREATED, DOCUMENT_VERSION_REBASED, DOCUMENT_VERSION_UPDATED, PreparedManifest,
    RepositoryError, VersionCommandIdentity, VersionMutationRecord, VersionOperationId,
    VersionOperationKind, VersionOperationRecord, VersionOperationResult,
};
use document_domain::{
    DocumentId, DocumentVersionId, LogicalPath, PrincipalRef, SemanticContentItem, Title,
    VersionManifest,
};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::error::{map_commit_error, map_statement_error};

const AUDIT_SOURCE: &str = "urn:knowledge-platform:document-platform";

#[derive(Debug, FromRow)]
struct OperationRow {
    operation_id: Uuid,
    operation_kind: String,
    document_id: Uuid,
    target_document_version_id: Uuid,
    expected_document_revision: i64,
    actor_identity_provider: String,
    actor_principal_id: String,
    command_digest: Vec<u8>,
    result: Value,
    resulting_document_revision: i64,
}

#[derive(Debug, FromRow)]
struct VersionState {
    document_id: Uuid,
    version_no: i64,
    base_document_version_id: Option<Uuid>,
    lifecycle_state: String,
    requires_content_classification: bool,
    scheduled_publish_at: Option<time::OffsetDateTime>,
}

pub(crate) async fn get_operation(
    pool: &PgPool,
    operation_id: VersionOperationId,
) -> Result<Option<VersionOperationRecord>, RepositoryError> {
    let row = sqlx::query_as::<_, OperationRow>(
        "SELECT operation_id, operation_kind, document_id, target_document_version_id, \
                expected_document_revision, actor_identity_provider, actor_principal_id, \
                command_digest, result, resulting_document_revision \
         FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(operation_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(map_statement_error)?;
    row.map(map_operation).transpose()
}

async fn get_operation_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: VersionOperationId,
) -> Result<Option<VersionOperationRecord>, RepositoryError> {
    let row = sqlx::query_as::<_, OperationRow>(
        "SELECT operation_id, operation_kind, document_id, target_document_version_id, \
                expected_document_revision, actor_identity_provider, actor_principal_id, \
                command_digest, result, resulting_document_revision \
         FROM document_version_operations WHERE operation_id = $1",
    )
    .bind(operation_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?;
    row.map(map_operation).transpose()
}

fn map_operation(row: OperationRow) -> Result<VersionOperationRecord, RepositoryError> {
    let kind = match row.operation_kind.as_str() {
        "CREATE" => VersionOperationKind::Create,
        "UPDATE" => VersionOperationKind::Update,
        "REBASE" => VersionOperationKind::Rebase,
        _ => return Err(RepositoryError::Conflict),
    };
    let operation_id = VersionOperationId::try_from_uuid(row.operation_id)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let actor = PrincipalRef::new(row.actor_identity_provider, row.actor_principal_id)
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let command_digest: [u8; 32] = row
        .command_digest
        .try_into()
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let document_id = DocumentId::from_uuid(row.document_id);
    let target_id = DocumentVersionId::from_uuid(row.target_document_version_id);
    let version_no = row
        .result
        .get("version_no")
        .and_then(Value::as_i64)
        .ok_or(RepositoryError::IntegrityViolation)?;
    let base_id = row
        .result
        .get("base_version_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(RepositoryError::IntegrityViolation)?;
    let identity = VersionCommandIdentity::from_persisted(
        operation_id,
        kind,
        document_id,
        target_id,
        row.expected_document_revision,
        actor,
        command_digest,
    );
    let result = VersionOperationResult::from_persisted(
        operation_id,
        document_id,
        target_id,
        version_no,
        DocumentVersionId::from_uuid(base_id),
        row.resulting_document_revision,
    );
    Ok(VersionOperationRecord::new(identity, result))
}

pub(crate) async fn mutate(
    pool: &PgPool,
    record: VersionMutationRecord,
) -> Result<VersionOperationResult, RepositoryError> {
    let identity = record.identity();
    let mut tx = pool.begin().await.map_err(map_statement_error)?;
    let result: Result<VersionOperationResult, RepositoryError> = async {
        let document = sqlx::query(
            "SELECT revision, current_version_id FROM documents WHERE document_id = $1 FOR UPDATE",
        )
        .bind(identity.document_id().as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::DocumentNotFound)?;

        if let Some(stored) = get_operation_in_tx(&mut tx, identity.operation_id()).await? {
            return if stored.matches_identity(identity) {
                Ok(stored.result().clone())
            } else {
                Err(RepositoryError::Conflict)
            };
        }

        let revision: i64 = document.get("revision");
        let current_id: Option<Uuid> = document.get("current_version_id");
        if revision != identity.expected_revision()
            || current_id != Some(record.expected_current_version_id().as_uuid())
        {
            return Err(RepositoryError::Conflict);
        }
        let current_id = current_id.ok_or(RepositoryError::BusinessRule)?;
        let current_state = load_version_state(&mut tx, current_id).await?
            .ok_or(RepositoryError::IntegrityViolation)?;
        if current_state.document_id != identity.document_id().as_uuid()
            || current_state.lifecycle_state != "PUBLISHED"
            || current_state.requires_content_classification
        {
            return Err(RepositoryError::BusinessRule);
        }
        let current_manifest = load_manifest(&mut tx, current_id).await?;
        if current_manifest.identity_digest() != record.expected_current_manifest_digest() {
            return Err(RepositoryError::Conflict);
        }
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
             WHERE document_id = $1 AND status = 'PENDING')",
        )
        .bind(identity.document_id().as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        if pending { return Err(RepositoryError::BusinessRule); }

        let target_id = identity.target_version_id().as_uuid();
        let (version_no, candidate) = match identity.kind() {
            VersionOperationKind::Create => {
                let existing_working: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM document_versions \
                     WHERE document_id = $1 AND lifecycle_state = 'WORKING')",
                )
                .bind(identity.document_id().as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                if existing_working { return Err(RepositoryError::BusinessRule); }
                let prepared = record.prepared().ok_or(RepositoryError::IntegrityViolation)?;
                validate_prepared(&mut tx, prepared).await?;
                let version_no: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(max(version_no), 0) + 1 FROM document_versions WHERE document_id = $1",
                )
                .bind(identity.document_id().as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(map_statement_error)?;
                if version_no <= 1 { return Err(RepositoryError::BusinessRule); }
                (version_no, prepared.manifest().clone())
            }
            VersionOperationKind::Update | VersionOperationKind::Rebase => {
                let target = load_version_state(&mut tx, target_id).await?
                    .ok_or(RepositoryError::DocumentVersionNotFound)?;
                if target.document_id != identity.document_id().as_uuid() {
                    return Err(RepositoryError::IntegrityViolation);
                }
                if target.lifecycle_state != "WORKING" || target.requires_content_classification
                    || target.scheduled_publish_at.is_some() {
                    return Err(RepositoryError::BusinessRule);
                }
                match identity.kind() {
                    VersionOperationKind::Update => {
                        if target.base_document_version_id != Some(current_id) {
                            return Err(RepositoryError::Conflict);
                        }
                        let prepared = record.prepared().ok_or(RepositoryError::IntegrityViolation)?;
                        validate_prepared(&mut tx, prepared).await?;
                        (target.version_no, prepared.manifest().clone())
                    }
                    VersionOperationKind::Rebase => {
                        if target.base_document_version_id == Some(current_id) {
                            return Err(RepositoryError::BusinessRule);
                        }
                        (target.version_no, load_manifest(&mut tx, target_id).await?)
                    }
                    VersionOperationKind::Create => unreachable!(),
                }
            }
        };
        ensure_semantic_change(&current_manifest, &candidate)?;
        let next_revision = revision.checked_add(1).ok_or(RepositoryError::IntegrityViolation)?;

        match identity.kind() {
            VersionOperationKind::Create => {
                sqlx::query(
                    "INSERT INTO document_versions \
                     (document_version_id, document_id, version_no, base_document_version_id, \
                      lifecycle_state, title, created_by_identity_provider, created_by_principal_id, \
                      metadata, created_at) \
                     VALUES ($1, $2, $3, $4, 'WORKING', $5, $6, $7, '{}'::jsonb, $8)",
                )
                .bind(target_id).bind(identity.document_id().as_uuid()).bind(version_no)
                .bind(current_id).bind(candidate.title().as_str())
                .bind(identity.actor().identity_provider()).bind(identity.actor().principal_id())
                .bind(record.occurred_at()).execute(&mut *tx).await.map_err(map_mutation_error)?;
                insert_content_items(&mut tx, target_id, record.prepared().ok_or(RepositoryError::IntegrityViolation)?).await?;
            }
            VersionOperationKind::Update => {
                sqlx::query("DELETE FROM content_items WHERE document_version_id = $1")
                    .bind(target_id).execute(&mut *tx).await.map_err(map_statement_error)?;
                sqlx::query("UPDATE document_versions SET title = $1 WHERE document_version_id = $2")
                    .bind(candidate.title().as_str()).bind(target_id)
                    .execute(&mut *tx).await.map_err(map_statement_error)?;
                insert_content_items(&mut tx, target_id, record.prepared().ok_or(RepositoryError::IntegrityViolation)?).await?;
            }
            VersionOperationKind::Rebase => {
                sqlx::query("UPDATE document_versions SET base_document_version_id = $1 WHERE document_version_id = $2")
                    .bind(current_id).bind(target_id).execute(&mut *tx).await.map_err(map_statement_error)?;
            }
        }

        sqlx::query("UPDATE documents SET revision = $1 WHERE document_id = $2")
            .bind(next_revision).bind(identity.document_id().as_uuid())
            .execute(&mut *tx).await.map_err(map_statement_error)?;

        let result = VersionOperationResult::from_persisted(
            identity.operation_id(), identity.document_id(), identity.target_version_id(),
            version_no, DocumentVersionId::from_uuid(current_id), next_revision,
        );
        let result_json = json!({"version_no": version_no, "base_version_id": current_id.to_string()});
        sqlx::query(
            "INSERT INTO document_version_operations \
             (operation_id, operation_kind, document_id, target_document_version_id, \
              expected_document_revision, actor_identity_provider, actor_principal_id, \
              command_digest, result, resulting_document_revision, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(identity.operation_id().as_uuid()).bind(identity.kind().as_str())
        .bind(identity.document_id().as_uuid()).bind(target_id)
        .bind(identity.expected_revision()).bind(identity.actor().identity_provider())
        .bind(identity.actor().principal_id()).bind(identity.command_digest().to_vec())
        .bind(result_json).bind(next_revision).bind(record.occurred_at())
        .execute(&mut *tx).await.map_err(map_mutation_error)?;
        insert_events(&mut tx, &record, &result).await?;
        Ok(result)
    }.await;

    match result {
        Ok(result) => {
            tx.commit().await.map_err(map_commit_error)?;
            Ok(result)
        }
        Err(error) => {
            let _ = tx.rollback().await;
            Err(error)
        }
    }
}

async fn load_version_state(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
) -> Result<Option<VersionState>, RepositoryError> {
    sqlx::query_as::<_, VersionState>(
        "SELECT document_id, version_no, base_document_version_id, lifecycle_state, \
                requires_content_classification, scheduled_publish_at \
         FROM document_versions WHERE document_version_id = $1",
    )
    .bind(version_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)
}

pub(crate) async fn load_manifest(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
) -> Result<VersionManifest, RepositoryError> {
    let version = sqlx::query(
        "SELECT title, requires_content_classification FROM document_versions WHERE document_version_id = $1",
    )
    .bind(version_id).fetch_optional(&mut **tx).await.map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentVersionNotFound)?;
    if version.get::<bool, _>("requires_content_classification") {
        return Err(RepositoryError::BusinessRule);
    }
    let title = Title::new(version.get::<String, _>("title"))
        .map_err(|_| RepositoryError::IntegrityViolation)?;
    let rows = sqlx::query(
        "SELECT ci.logical_path, ci.ordinal, cr.detected_format AS representation_format, \
                cr.inspection_profile_version AS representation_profile, \
                cr.semantic_fingerprint AS representation_fingerprint, \
                s.detected_format, s.inspection_profile_version, s.fingerprint_digest, \
                s.observed_raw_content_hash, s.observed_size_bytes, \
                f.content_hash, f.size_bytes \
         FROM content_items ci \
         JOIN content_representations cr ON cr.content_representation_id = ci.authoritative_representation_id \
            AND cr.content_item_id = ci.content_item_id AND cr.role = 'AUTHORITATIVE' \
         JOIN file_objects f ON f.file_id = cr.file_id \
         LEFT JOIN document_semantic_inspections s ON s.file_id = f.file_id \
            AND s.inspection_profile_version = 'dsi-v0' \
         WHERE ci.document_version_id = $1 ORDER BY ci.ordinal, ci.logical_path",
    )
    .bind(version_id).fetch_all(&mut **tx).await.map_err(map_statement_error)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let path: String = row.get("logical_path");
        let ordinal: i32 = row.get("ordinal");
        let format: Option<String> = row.get("detected_format");
        let profile: Option<String> = row.get("inspection_profile_version");
        let digest: Option<Vec<u8>> = row.get("fingerprint_digest");
        let raw: Option<Vec<u8>> = row.get("observed_raw_content_hash");
        let size: Option<i64> = row.get("observed_size_bytes");
        let format = format.ok_or(RepositoryError::IntegrityViolation)?;
        let profile = profile.ok_or(RepositoryError::IntegrityViolation)?;
        let digest = digest.ok_or(RepositoryError::IntegrityViolation)?;
        if raw != Some(row.get::<Vec<u8>, _>("content_hash"))
            || size != Some(row.get::<i64, _>("size_bytes"))
            || row
                .get::<Option<String>, _>("representation_format")
                .is_some_and(|v| v != format)
            || row
                .get::<Option<String>, _>("representation_profile")
                .is_some_and(|v| v != profile)
            || row
                .get::<Option<Vec<u8>>, _>("representation_fingerprint")
                .is_some_and(|v| v != digest)
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        let digest: [u8; 32] = digest
            .try_into()
            .map_err(|_| RepositoryError::IntegrityViolation)?;
        let ordinal = u32::try_from(ordinal).map_err(|_| RepositoryError::IntegrityViolation)?;
        items.push(
            SemanticContentItem::new(
                LogicalPath::new(&path).map_err(|_| RepositoryError::IntegrityViolation)?,
                ordinal,
                format,
                profile,
                digest,
            )
            .map_err(|_| RepositoryError::IntegrityViolation)?,
        );
    }
    VersionManifest::new(title, items).map_err(|_| RepositoryError::IntegrityViolation)
}

pub(crate) fn ensure_semantic_change(
    base: &VersionManifest,
    candidate: &VersionManifest,
) -> Result<(), RepositoryError> {
    for old in base.items() {
        if let Some(new) = candidate.items().iter().find(|item| {
            item.logical_path() == old.logical_path() && item.ordinal() == old.ordinal()
        }) && (new.format_id() != old.format_id()
            || new.inspection_profile_id() != old.inspection_profile_id())
        {
            return Err(RepositoryError::BusinessRule);
        }
    }
    if base.identity_digest() == candidate.identity_digest() {
        return Err(RepositoryError::BusinessRule);
    }
    Ok(())
}

async fn validate_prepared(
    tx: &mut Transaction<'_, Postgres>,
    prepared: &PreparedManifest,
) -> Result<(), RepositoryError> {
    for item in prepared.items() {
        let file = item.file();
        let response = item.inspection().response();
        let row = sqlx::query(
            "SELECT f.content_hash, f.size_bytes, f.media_type, f.storage_locator, \
                    s.observed_raw_content_hash, s.observed_size_bytes, \
                    s.detected_format, s.fingerprint_digest \
             FROM file_objects f \
             JOIN document_semantic_inspections s ON s.file_id = f.file_id \
               AND s.inspection_profile_version = 'dsi-v0' \
             WHERE f.file_id = $1",
        )
        .bind(file.file_id().as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_statement_error)?
        .ok_or(RepositoryError::IntegrityViolation)?;
        if row.get::<Vec<u8>, _>("content_hash") != file.content_hash().as_bytes()
            || row.get::<i64, _>("size_bytes") != file.size_bytes().get()
            || row.get::<String, _>("media_type") != file.media_type().as_str()
            || row.get::<String, _>("storage_locator") != file.storage_key().as_str()
            || row.get::<Vec<u8>, _>("observed_raw_content_hash") != file.content_hash().as_bytes()
            || row.get::<i64, _>("observed_size_bytes") != file.size_bytes().get()
            || row.get::<String, _>("detected_format") != format_id(response.detected_format)
            || row.get::<Vec<u8>, _>("fingerprint_digest") != response.semantic_fingerprint.digest()
            || response.observed_raw_content_hash != *file.content_hash().as_bytes()
            || response.observed_size_bytes != file.size_bytes().get() as u64
        {
            return Err(RepositoryError::IntegrityViolation);
        }
        for rendition in item.renditions() {
            validate_file_binding(tx, rendition.file()).await?;
        }
    }
    Ok(())
}

async fn validate_file_binding(
    tx: &mut Transaction<'_, Postgres>,
    file: &document_domain::FileObject,
) -> Result<(), RepositoryError> {
    let row = sqlx::query("SELECT content_hash, size_bytes, media_type, storage_locator FROM file_objects WHERE file_id = $1")
        .bind(file.file_id().as_uuid()).fetch_optional(&mut **tx).await.map_err(map_statement_error)?
        .ok_or(RepositoryError::IntegrityViolation)?;
    if row.get::<Vec<u8>, _>("content_hash") != file.content_hash().as_bytes()
        || row.get::<i64, _>("size_bytes") != file.size_bytes().get()
        || row.get::<String, _>("media_type") != file.media_type().as_str()
        || row.get::<String, _>("storage_locator") != file.storage_key().as_str()
    {
        return Err(RepositoryError::IntegrityViolation);
    }
    Ok(())
}

async fn insert_content_items(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
    prepared: &PreparedManifest,
) -> Result<(), RepositoryError> {
    for item in prepared.items() {
        let item_id = Uuid::now_v7();
        let representation_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO content_items \
             (content_item_id, document_version_id, logical_path, ordinal, authoritative_representation_id) \
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(item_id).bind(version_id).bind(item.logical_path().as_str())
        .bind(i32::try_from(item.ordinal()).map_err(|_| RepositoryError::BusinessRule)?)
        .bind(representation_id).execute(&mut **tx).await.map_err(map_mutation_error)?;
        sqlx::query(
            "INSERT INTO content_representations \
             (content_representation_id, content_item_id, file_id, role, original_filename, \
              detected_format, inspection_profile_version, semantic_fingerprint) \
             VALUES ($1,$2,$3,'AUTHORITATIVE',$4,$5,$6,$7)",
        )
        .bind(representation_id)
        .bind(item_id)
        .bind(item.file().file_id().as_uuid())
        .bind(item.original_filename())
        .bind(format_id(item.inspection().response().detected_format))
        .bind(
            item.inspection()
                .response()
                .inspection_profile_version
                .as_str(),
        )
        .bind(
            item.inspection()
                .response()
                .semantic_fingerprint
                .digest()
                .to_vec(),
        )
        .execute(&mut **tx)
        .await
        .map_err(map_mutation_error)?;
        for rendition in item.renditions() {
            sqlx::query(
                "INSERT INTO content_representations \
                 (content_representation_id, content_item_id, file_id, role, original_filename) \
                 VALUES ($1,$2,$3,'RENDITION',$4)",
            )
            .bind(Uuid::now_v7())
            .bind(item_id)
            .bind(rendition.file().file_id().as_uuid())
            .bind(rendition.original_filename())
            .execute(&mut **tx)
            .await
            .map_err(map_mutation_error)?;
        }
    }
    Ok(())
}

async fn insert_events(
    tx: &mut Transaction<'_, Postgres>,
    record: &VersionMutationRecord,
    result: &VersionOperationResult,
) -> Result<(), RepositoryError> {
    let identity = record.identity();
    let (event_type, audit_type) = match identity.kind() {
        VersionOperationKind::Create => (DOCUMENT_VERSION_CREATED, AUDIT_DOCUMENT_VERSION_CREATED),
        VersionOperationKind::Update => (DOCUMENT_VERSION_UPDATED, AUDIT_DOCUMENT_VERSION_UPDATED),
        VersionOperationKind::Rebase => (DOCUMENT_VERSION_REBASED, AUDIT_DOCUMENT_VERSION_REBASED),
    };
    let payload = json!({
        "documentId": identity.document_id().as_uuid().to_string(),
        "documentVersionId": identity.target_version_id().as_uuid().to_string(),
        "versionNo": result.version_no(),
        "baseDocumentVersionId": result.base_version_id().as_uuid().to_string(),
        "resultingDocumentRevision": result.resulting_revision(),
    });
    sqlx::query(
        "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at,attempt_count,delivered_at) \
         VALUES ($1,$2,'Document',$3,$4,$5,$5,0,NULL)",
    )
    .bind(record.domain_event_id().as_uuid()).bind(event_type)
    .bind(identity.document_id().as_uuid()).bind(payload.clone()).bind(record.occurred_at())
    .execute(&mut **tx).await.map_err(map_mutation_error)?;
    let subject = format!(
        "document/{}/version/{}",
        identity.document_id().as_uuid(),
        identity.target_version_id().as_uuid()
    );
    sqlx::query(
        "INSERT INTO audit_outbox_events \
         (event_id,event_type,source,subject,actor_identity_provider,actor_principal_id, \
          resource_id,resource_version_id,result,trace_id,data,occurred_at,attempt_count,delivered_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'success',NULL,$9,$10,0,NULL)",
    )
    .bind(record.audit_event_id().as_uuid()).bind(audit_type).bind(AUDIT_SOURCE).bind(subject)
    .bind(identity.actor().identity_provider()).bind(identity.actor().principal_id())
    .bind(identity.document_id().as_uuid()).bind(identity.target_version_id().as_uuid())
    .bind(payload).bind(record.occurred_at())
    .execute(&mut **tx).await.map_err(map_mutation_error)?;
    Ok(())
}

fn map_mutation_error(error: sqlx::Error) -> RepositoryError {
    if let sqlx::Error::Database(database) = &error
        && matches!(
            database.code().as_deref(),
            Some("23505" | "23503" | "23514")
        )
    {
        return RepositoryError::Conflict;
    }
    map_statement_error(error)
}

fn format_id(format: document_semantic_inspection_core::FormatId) -> &'static str {
    use document_semantic_inspection_core::FormatId;
    match format {
        FormatId::Docx => "docx",
        FormatId::Xlsx => "xlsx",
        FormatId::Xlsm => "xlsm",
        FormatId::Pptx => "pptx",
        FormatId::Pdf => "pdf",
        FormatId::Txt => "txt",
        FormatId::Csv => "csv",
        FormatId::Html => "html",
    }
}
