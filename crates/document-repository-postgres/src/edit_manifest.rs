use document_application::{
    EditManifest, EditManifestItem, EditManifestRepository, EditManifestRepresentation,
    EditManifestRequest, EditManifestRole, RepositoryError, VerifiedActorContext,
};
use document_domain::{Action, ResourceRef};
use sqlx::Row;

use crate::{
    PostgresDocumentRepository,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    document_history::{authorize_version_in_tx, begin_snapshot},
    error::map_statement_error,
};

impl EditManifestRepository for PostgresDocumentRepository {
    async fn get_edit_manifest(
        &self,
        ctx: &VerifiedActorContext,
        request: EditManifestRequest,
    ) -> Result<EditManifest, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let version = authorize_version_in_tx(&mut tx, ctx, request.into()).await?;
        match authorize_in_tx(
            &mut tx,
            ctx,
            &[(
                ResourceRef::Document(request.document_id),
                vec![Action::Read, Action::Write],
            )],
        )
        .await
        {
            Err(RepositoryError::Forbidden) => {
                return Err(RepositoryError::DocumentVersionNotFound);
            }
            other => other?,
        }
        let rows = sqlx::query(
            "SELECT item.content_item_id,item.logical_path,item.ordinal, \
                    rep.content_representation_id,rep.role,rep.file_id,rep.original_filename, \
                    file.media_type,file.size_bytes \
             FROM content_items item \
             JOIN content_representations rep ON rep.content_item_id=item.content_item_id \
             JOIN file_objects file ON file.file_id=rep.file_id \
             WHERE item.document_version_id=$1 \
             ORDER BY item.ordinal,item.logical_path, \
                      CASE rep.role WHEN 'AUTHORITATIVE' THEN 0 ELSE 1 END, \
                      rep.content_representation_id",
        )
        .bind(request.source_version_id.as_uuid())
        .fetch_all(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        let mut items: Vec<EditManifestItem> = Vec::new();
        for row in rows {
            let content_item_id = row
                .try_get("content_item_id")
                .map_err(map_statement_error)?;
            if items
                .last()
                .is_none_or(|item| item.content_item_id != content_item_id)
            {
                items.push(EditManifestItem {
                    content_item_id,
                    logical_path: row.try_get("logical_path").map_err(map_statement_error)?,
                    ordinal: row.try_get("ordinal").map_err(map_statement_error)?,
                    representations: Vec::new(),
                });
            }
            let role: String = row.try_get("role").map_err(map_statement_error)?;
            let representation = EditManifestRepresentation {
                representation_id: row
                    .try_get("content_representation_id")
                    .map_err(map_statement_error)?,
                role: match role.as_str() {
                    "AUTHORITATIVE" => EditManifestRole::Authoritative,
                    "RENDITION" => EditManifestRole::Rendition,
                    _ => return Err(RepositoryError::IntegrityViolation),
                },
                file_id: row.try_get("file_id").map_err(map_statement_error)?,
                original_filename: row
                    .try_get("original_filename")
                    .map_err(map_statement_error)?,
                media_type: row.try_get("media_type").map_err(map_statement_error)?,
                size_bytes: row.try_get("size_bytes").map_err(map_statement_error)?,
            };
            items
                .last_mut()
                .ok_or(RepositoryError::IntegrityViolation)?
                .representations
                .push(representation);
        }
        ensure_editable_manifest(
            version
                .try_get("requires_content_classification")
                .map_err(map_statement_error)?,
            &items,
        )?;
        let manifest = EditManifest {
            document_id: request.document_id,
            source_version_id: request.source_version_id,
            document_revision: version
                .try_get("document_revision")
                .map_err(map_statement_error)?,
            purpose: request.purpose,
            title: version.try_get("title").map_err(map_statement_error)?,
            items,
        };
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(manifest)
    }
}

fn ensure_editable_manifest(
    requires_classification: bool,
    items: &[EditManifestItem],
) -> Result<(), RepositoryError> {
    if requires_classification || items.is_empty() {
        return Err(RepositoryError::BusinessRule);
    }
    for item in items {
        if item
            .representations
            .first()
            .is_none_or(|representation| representation.role != EditManifestRole::Authoritative)
            || item
                .representations
                .iter()
                .skip(1)
                .any(|representation| representation.role != EditManifestRole::Rendition)
        {
            return Err(RepositoryError::IntegrityViolation);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn item(roles: &[EditManifestRole]) -> EditManifestItem {
        EditManifestItem {
            content_item_id: Uuid::from_u128(1),
            logical_path: "primary".into(),
            ordinal: 0,
            representations: roles
                .iter()
                .enumerate()
                .map(|(index, role)| EditManifestRepresentation {
                    representation_id: Uuid::from_u128(index as u128 + 10),
                    role: *role,
                    file_id: Uuid::from_u128(index as u128 + 20),
                    original_filename: "../source.txt".into(),
                    media_type: "text/plain".into(),
                    size_bytes: 7,
                })
                .collect(),
        }
    }

    #[test]
    fn legacy_or_empty_edit_manifest_fails_closed() {
        let valid = item(&[EditManifestRole::Authoritative]);
        assert_eq!(
            ensure_editable_manifest(true, &[valid]),
            Err(RepositoryError::BusinessRule)
        );
        assert_eq!(
            ensure_editable_manifest(false, &[]),
            Err(RepositoryError::BusinessRule)
        );
    }

    #[test]
    fn each_edit_item_requires_one_authoritative_representation_before_renditions() {
        for roles in [
            vec![],
            vec![EditManifestRole::Rendition],
            vec![
                EditManifestRole::Authoritative,
                EditManifestRole::Authoritative,
            ],
            vec![EditManifestRole::Rendition, EditManifestRole::Authoritative],
        ] {
            assert_eq!(
                ensure_editable_manifest(false, &[item(&roles)]),
                Err(RepositoryError::IntegrityViolation)
            );
        }
        assert_eq!(
            ensure_editable_manifest(
                false,
                &[item(&[
                    EditManifestRole::Authoritative,
                    EditManifestRole::Rendition
                ])]
            ),
            Ok(())
        );
    }
}
