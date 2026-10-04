//! Targeted historical Document lookup. It never enumerates or falls back from Live.

use std::sync::Arc;

use document_application::{
    ApplicationError, DocumentHistoryService, VerifiedActorContext, VersionPurpose, VersionRequest,
};
use document_domain::{DocumentId, DocumentVersionId};
use document_repository_postgres::PostgresDocumentRepository;
use search_application::SearchError;

use crate::postgres::{DocumentSnapshotReader, PostgresDocumentSnapshotReader};
use crate::translate::{DocumentSourceTranslation, DocumentSourceTranslator};

pub struct DocumentHistoricalLookup {
    reader: PostgresDocumentSnapshotReader,
    history: DocumentHistoryService<PostgresDocumentRepository>,
    actor: VerifiedActorContext,
    translator: DocumentSourceTranslator,
}

impl DocumentHistoricalLookup {
    pub fn new(
        reader: PostgresDocumentSnapshotReader,
        repository: Arc<PostgresDocumentRepository>,
        actor: VerifiedActorContext,
        translator: DocumentSourceTranslator,
    ) -> Self {
        Self {
            reader,
            history: DocumentHistoryService::new(repository),
            actor,
            translator,
        }
    }

    /// Both explicit IDs and Document's `Read` + `ReadHistory` authorization
    /// are mandatory. The second check closes the gap after the D3 snapshot.
    pub async fn lookup(
        &self,
        document_id: DocumentId,
        version_id: DocumentVersionId,
    ) -> Result<Option<DocumentSourceTranslation>, SearchError> {
        let request = VersionRequest {
            document_id,
            document_version_id: version_id,
            purpose: VersionPurpose::History,
        };
        if !self.authorized(request).await? {
            return Ok(None);
        }
        let Some(record) = self
            .reader
            .load_document_version(version_id)
            .await
            .map_err(|error| SearchError::SourceUnavailable(error.to_string()))?
        else {
            return Ok(None);
        };
        if record.snapshot.document_id != document_id
            || record.snapshot.document_version_id != version_id
            || (record.snapshot.current_version_id == Some(version_id)
                && record.snapshot.publication_end.is_none())
        {
            return Ok(None);
        }
        let translation = self
            .translator
            .translate_record(record)
            .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
        if !matches!(translation, DocumentSourceTranslation::Historical { .. })
            || !self.authorized(request).await?
        {
            return Ok(None);
        }
        Ok(Some(translation))
    }

    async fn authorized(&self, request: VersionRequest) -> Result<bool, SearchError> {
        match self
            .history
            .get_document_version(&self.actor, request)
            .await
        {
            Ok(_) => Ok(true),
            Err(
                ApplicationError::Forbidden
                | ApplicationError::DocumentNotFound
                | ApplicationError::DocumentVersionNotFound
                | ApplicationError::StaleVersion,
            ) => Ok(false),
            Err(error) => Err(SearchError::SourceUnavailable(error.to_string())),
        }
    }
}
