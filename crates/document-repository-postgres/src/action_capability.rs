use document_application::{
    ActionAvailability, ActionCapabilityReadRepository, CapabilityDisabledReason,
    DocumentActionCapabilities, FolderActionCapabilities, InvocationKind, RepositoryError,
    VerifiedActorContext, VersionActionCapabilities, VersionPurpose, VersionRequest,
};
use document_domain::{Action, DocumentId, FolderId, ResourceRef};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::{
    PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID,
    access_control::{AccessLockMode, authorize_in_tx, lock_access_state},
    document_history::{authorize_version_in_tx, begin_snapshot},
    error::map_statement_error,
};

fn availability(
    permitted: bool,
    human_required: bool,
    is_human: bool,
    state_reason: Option<CapabilityDisabledReason>,
) -> ActionAvailability {
    if !permitted {
        return ActionAvailability::disabled(CapabilityDisabledReason::Permission);
    }
    if human_required && !is_human {
        return ActionAvailability::disabled(CapabilityDisabledReason::NotHumanInteractive);
    }
    if let Some(reason) = state_reason {
        return ActionAvailability::disabled(reason);
    }
    ActionAvailability::available()
}

struct CurrentPublicationState {
    published: bool,
    requires_content_classification: bool,
}

impl CurrentPublicationState {
    fn from_row(row: &PgRow) -> Result<Self, RepositoryError> {
        Ok(Self {
            published: row
                .try_get("has_current_published")
                .map_err(map_statement_error)?,
            requires_content_classification: row
                .try_get("current_requires_content_classification")
                .map_err(map_statement_error)?,
        })
    }

    fn has_classified_published(&self) -> bool {
        self.published && !self.requires_content_classification
    }

    fn end_publication_reason(&self, ended: bool) -> Option<CapabilityDisabledReason> {
        if ended {
            Some(CapabilityDisabledReason::Lifecycle)
        } else if !self.published {
            // T10 removes publication without restoring or classifying content.
            Some(CapabilityDisabledReason::NotCurrent)
        } else {
            None
        }
    }
}

async fn permits(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &VerifiedActorContext,
    resource: ResourceRef,
    actions: &[Action],
) -> Result<bool, RepositoryError> {
    match authorize_in_tx(tx, ctx, &[(resource, actions.to_vec())]).await {
        Ok(()) => Ok(true),
        Err(RepositoryError::Forbidden) => Ok(false),
        Err(error) => Err(error),
    }
}

async fn document_state(
    tx: &mut Transaction<'_, Postgres>,
    document_id: DocumentId,
) -> Result<PgRow, RepositoryError> {
    sqlx::query(
        "SELECT d.folder_id, d.current_version_id, \
                EXISTS (SELECT 1 FROM document_publication_end_operations ended \
                        WHERE ended.document_id = d.document_id) AS ended, \
                EXISTS (SELECT 1 FROM document_publish_schedules schedule \
                        WHERE schedule.document_id = d.document_id \
                          AND schedule.status = 'PENDING') AS pending_schedule, \
                EXISTS (SELECT 1 FROM document_versions version \
                        WHERE version.document_id = d.document_id \
                          AND version.lifecycle_state = 'WORKING') AS has_working_version, \
                EXISTS (SELECT 1 FROM document_versions version \
                        WHERE version.document_id = d.document_id \
                          AND version.document_version_id = d.current_version_id \
                          AND version.lifecycle_state = 'PUBLISHED') AS has_current_published, \
                EXISTS (SELECT 1 FROM document_versions version \
                        WHERE version.document_id = d.document_id \
                          AND version.document_version_id = d.current_version_id \
                          AND version.requires_content_classification) AS current_requires_content_classification, \
                (SELECT count(*)::bigint FROM document_versions version \
                 WHERE version.document_id = d.document_id) AS version_count \
         FROM documents d WHERE d.document_id = $1",
    )
    .bind(document_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(map_statement_error)?
    .ok_or(RepositoryError::DocumentNotFound)
}

impl ActionCapabilityReadRepository for PostgresDocumentRepository {
    async fn read_document_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        document_id: DocumentId,
    ) -> Result<DocumentActionCapabilities, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let state = document_state(&mut tx, document_id).await?;
        let document = ResourceRef::Document(document_id);
        if !permits(&mut tx, ctx, document, &[Action::Read]).await? {
            return Err(RepositoryError::DocumentNotFound);
        }
        let source_folder =
            FolderId::from_uuid(state.try_get("folder_id").map_err(map_statement_error)?);
        let ended: bool = state.try_get("ended").map_err(map_statement_error)?;
        let pending: bool = state
            .try_get("pending_schedule")
            .map_err(map_statement_error)?;
        let has_working: bool = state
            .try_get("has_working_version")
            .map_err(map_statement_error)?;
        let current_publication = CurrentPublicationState::from_row(&state)?;
        let has_classified_current_published = current_publication.has_classified_published();
        let version_count: i64 = state
            .try_get("version_count")
            .map_err(map_statement_error)?;
        let is_human = ctx.invocation_kind() == InvocationKind::HumanInteractive;

        let write = permits(&mut tx, ctx, document, &[Action::Read, Action::Write]).await?;
        let metadata_permissions = if ended {
            permits(
                &mut tx,
                ctx,
                document,
                &[
                    Action::Read,
                    Action::Write,
                    Action::ReadHistory,
                    Action::Administer,
                ],
            )
            .await?
        } else {
            write
        };
        let mut move_actions = vec![Action::Read, Action::Write, Action::Administer];
        if ended {
            move_actions.push(Action::ReadHistory);
        }
        let move_document = permits(&mut tx, ctx, document, &move_actions).await?
            && permits(
                &mut tx,
                ctx,
                ResourceRef::Folder(source_folder),
                &[Action::Administer],
            )
            .await?;
        let publish = permits(&mut tx, ctx, document, &[Action::Read, Action::Publish]).await?;
        let administer = permits(&mut tx, ctx, document, &[Action::Administer]).await?;
        let compare = permits(&mut tx, ctx, document, &[Action::Read, Action::ReadHistory]).await?;
        let pending_reason = pending.then_some(CapabilityDisabledReason::PendingSchedule);
        let end_publication_reason = current_publication.end_publication_reason(ended);
        let capabilities = DocumentActionCapabilities {
            create_version: availability(
                write,
                true,
                is_human,
                pending_reason
                    .or_else(|| {
                        (ended || has_working).then_some(CapabilityDisabledReason::Lifecycle)
                    })
                    .or_else(|| {
                        (!has_classified_current_published)
                            .then_some(CapabilityDisabledReason::NotCurrent)
                    }),
            ),
            update_metadata: availability(metadata_permissions, true, is_human, pending_reason),
            move_document: availability(move_document, true, is_human, pending_reason),
            end_publication: availability(
                publish,
                true,
                is_human,
                pending_reason.or(end_publication_reason),
            ),
            manage_access: availability(administer, true, is_human, None),
            compare_versions: availability(
                compare,
                false,
                is_human,
                (version_count < 2).then_some(CapabilityDisabledReason::Lifecycle),
            ),
        };
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(capabilities)
    }

    async fn read_version_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<VersionActionCapabilities, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let row = authorize_version_in_tx(&mut tx, ctx, request).await?;
        let lifecycle: String = row
            .try_get("lifecycle_state")
            .map_err(map_statement_error)?;
        let current: Option<Uuid> = row
            .try_get("current_version_id")
            .map_err(map_statement_error)?;
        let ended: bool = row.try_get("ended").map_err(map_statement_error)?;
        let version_no: i64 = row.try_get("version_no").map_err(map_statement_error)?;
        let base: Option<Uuid> = row
            .try_get("base_document_version_id")
            .map_err(map_statement_error)?;
        let document_version_id = request.document_version_id.as_uuid();
        let requires_classification: bool = row
            .try_get("requires_content_classification")
            .map_err(map_statement_error)?;
        let scheduled: Option<time::OffsetDateTime> = row
            .try_get("scheduled_publish_at")
            .map_err(map_statement_error)?;
        let state = document_state(&mut tx, request.document_id).await?;
        let current_publication = CurrentPublicationState::from_row(&state)?;
        let has_classified_current_published = current_publication.has_classified_published();
        let initial = version_no == 1
            && current.is_none()
            && base.is_none()
            && !crate::versioning_mutation::has_publication_history(&mut tx, request.document_id)
                .await?;
        let stale_base = lifecycle == "WORKING"
            && if version_no == 1 {
                !initial
            } else {
                current.is_none() || base != current
            };
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
             WHERE document_id = $1 AND status = 'PENDING')",
        )
        .bind(request.document_id.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        let pending_target: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM document_publish_schedules \
             WHERE document_id = $1 AND target_document_version_id = $2 \
               AND status = 'PENDING')",
        )
        .bind(request.document_id.as_uuid())
        .bind(document_version_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_statement_error)?;
        let resource = ResourceRef::Document(request.document_id);
        let write = permits(&mut tx, ctx, resource, &[Action::Read, Action::Write]).await?;
        let publish = permits(&mut tx, ctx, resource, &[Action::Read, Action::Publish]).await?;
        let download_actions = match request.purpose {
            VersionPurpose::Published => vec![Action::Read],
            VersionPurpose::Authoring => vec![Action::Read, Action::Write],
            VersionPurpose::History => vec![Action::Read, Action::ReadHistory],
        };
        let download = permits(&mut tx, ctx, resource, &download_actions).await?;
        let is_human = ctx.invocation_kind() == InvocationKind::HumanInteractive;
        let working = lifecycle == "WORKING";
        let published = lifecycle == "PUBLISHED";
        let pending_reason =
            (pending || scheduled.is_some()).then_some(CapabilityDisabledReason::PendingSchedule);
        let stale_reason = stale_base.then_some(CapabilityDisabledReason::StaleBase);
        let capabilities = VersionActionCapabilities {
            edit: availability(
                write,
                true,
                is_human,
                if !working
                    || ended
                    || requires_classification
                    || (current.is_some() && !has_classified_current_published)
                {
                    Some(CapabilityDisabledReason::Lifecycle)
                } else {
                    pending_reason.or(stale_reason)
                },
            ),
            rebase: availability(
                write,
                true,
                is_human,
                if !working || ended || requires_classification || !has_classified_current_published
                {
                    Some(CapabilityDisabledReason::Lifecycle)
                } else if pending_reason.is_some() {
                    pending_reason
                } else if stale_base {
                    None
                } else {
                    Some(CapabilityDisabledReason::Lifecycle)
                },
            ),
            publish: availability(
                publish,
                true,
                is_human,
                if !working || ended {
                    Some(CapabilityDisabledReason::Lifecycle)
                } else {
                    pending_reason.or(stale_reason)
                },
            ),
            withdraw: availability(
                publish,
                true,
                is_human,
                (!published).then_some(CapabilityDisabledReason::Lifecycle),
            ),
            schedule_publication: availability(
                publish,
                true,
                is_human,
                if !working || ended {
                    Some(CapabilityDisabledReason::Lifecycle)
                } else {
                    pending_reason.or(stale_reason)
                },
            ),
            cancel_publication_schedule: availability(
                publish,
                true,
                is_human,
                if pending_target {
                    None
                } else {
                    Some(CapabilityDisabledReason::Lifecycle)
                },
            ),
            download: availability(download, false, is_human, None),
        };
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(capabilities)
    }

    async fn read_folder_action_capabilities(
        &self,
        ctx: &VerifiedActorContext,
        folder_id: FolderId,
    ) -> Result<FolderActionCapabilities, RepositoryError> {
        let mut tx = begin_snapshot(&self.pool).await?;
        lock_access_state(&mut tx, AccessLockMode::Shared).await?;
        let status: String = sqlx::query_scalar("SELECT status FROM folders WHERE folder_id = $1")
            .bind(folder_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_statement_error)?
            .ok_or(RepositoryError::FolderNotFound)?;
        let resource = ResourceRef::Folder(folder_id);
        if !permits(&mut tx, ctx, resource, &[Action::Read]).await? {
            return Err(RepositoryError::FolderNotFound);
        }
        let write = permits(&mut tx, ctx, resource, &[Action::Read, Action::Write]).await?;
        let administer = permits(&mut tx, ctx, resource, &[Action::Administer]).await?;
        let active_reason = (status != "ACTIVE").then_some(CapabilityDisabledReason::Lifecycle);
        let is_root = folder_id.as_uuid() == SYSTEM_ROOT_FOLDER_ID;
        let unsupported_root = is_root.then_some(CapabilityDisabledReason::Unsupported);
        let is_human = ctx.invocation_kind() == InvocationKind::HumanInteractive;
        let capabilities = FolderActionCapabilities {
            create_document: availability(write, true, is_human, active_reason),
            create_folder: availability(administer, true, is_human, active_reason),
            rename_folder: availability(
                administer,
                true,
                is_human,
                unsupported_root.or(active_reason),
            ),
            move_folder: availability(
                administer,
                true,
                is_human,
                unsupported_root.or(active_reason),
            ),
            manage_access: availability(administer, true, is_human, active_reason),
        };
        tx.rollback().await.map_err(map_statement_error)?;
        Ok(capabilities)
    }
}

#[cfg(test)]
mod current_publication_tests {
    use super::{CurrentPublicationState, availability};
    use document_application::{ActionAvailability, CapabilityDisabledReason};

    #[test]
    fn unclassified_current_can_end_publication_but_cannot_be_a_versioning_base() {
        let current = CurrentPublicationState {
            published: true,
            requires_content_classification: true,
        };
        assert_eq!(
            availability(true, true, true, current.end_publication_reason(false)),
            ActionAvailability::available(),
        );
        assert!(!current.has_classified_published());
    }

    #[test]
    fn end_publication_preserves_ended_and_missing_current_guards() {
        for requires_content_classification in [false, true] {
            let current = CurrentPublicationState {
                published: false,
                requires_content_classification,
            };
            assert_eq!(
                current.end_publication_reason(false),
                Some(CapabilityDisabledReason::NotCurrent),
            );
            assert_eq!(
                current.end_publication_reason(true),
                Some(CapabilityDisabledReason::Lifecycle),
            );
            assert!(!current.has_classified_published());
        }
        let current = CurrentPublicationState {
            published: true,
            requires_content_classification: false,
        };
        assert_eq!(
            current.end_publication_reason(true),
            Some(CapabilityDisabledReason::Lifecycle),
        );
        assert_eq!(current.end_publication_reason(false), None);
        assert!(current.has_classified_published());
    }
}
