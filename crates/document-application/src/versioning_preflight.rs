use std::{collections::BTreeSet, sync::Arc};

use document_domain::{
    FileId, FileObject, LogicalPath, MediaType, SemanticContentItem, Title, VersionManifest,
};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};

use crate::{
    ApplicationError, AuthoritativeDocument, Clock, ContentReader, EnsureSemanticInspection,
    FileStorage, SemanticInspectionExecutor, SemanticInspectionRecord,
    SemanticInspectionRepository, StoreFileRequest, VersioningRepository,
    publish_quality::check_publish_quality,
};

pub struct VersioningRenditionInput {
    file_id: FileId,
    media_type: MediaType,
    original_filename: String,
    content: ContentReader,
}

impl VersioningRenditionInput {
    pub fn new(
        file_id: FileId,
        media_type: MediaType,
        original_filename: impl Into<String>,
        content: ContentReader,
    ) -> Self {
        Self {
            file_id,
            media_type,
            original_filename: original_filename.into(),
            content,
        }
    }
}

pub struct VersioningItemInput {
    logical_path: LogicalPath,
    ordinal: u32,
    file_id: FileId,
    media_type: MediaType,
    original_filename: String,
    content: ContentReader,
    renditions: Vec<VersioningRenditionInput>,
}

impl VersioningItemInput {
    pub fn new(
        logical_path: LogicalPath,
        ordinal: u32,
        file_id: FileId,
        media_type: MediaType,
        original_filename: impl Into<String>,
        content: ContentReader,
    ) -> Self {
        Self {
            logical_path,
            ordinal,
            file_id,
            media_type,
            original_filename: original_filename.into(),
            content,
            renditions: vec![],
        }
    }

    pub fn with_renditions(mut self, renditions: Vec<VersioningRenditionInput>) -> Self {
        self.renditions = renditions;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRendition {
    file: FileObject,
    original_filename: String,
}

impl PreparedRendition {
    pub const fn file(&self) -> &FileObject {
        &self.file
    }
    pub fn original_filename(&self) -> &str {
        &self.original_filename
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContentItem {
    logical_path: LogicalPath,
    ordinal: u32,
    file: FileObject,
    inspection: SemanticInspectionRecord,
    original_filename: String,
    renditions: Vec<PreparedRendition>,
}

impl PreparedContentItem {
    pub fn logical_path(&self) -> &LogicalPath {
        &self.logical_path
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub const fn file(&self) -> &FileObject {
        &self.file
    }

    pub const fn inspection(&self) -> &SemanticInspectionRecord {
        &self.inspection
    }

    pub fn original_filename(&self) -> &str {
        &self.original_filename
    }

    pub fn renditions(&self) -> &[PreparedRendition] {
        &self.renditions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedManifest {
    manifest: VersionManifest,
    items: Vec<PreparedContentItem>,
}

impl PreparedManifest {
    pub fn manifest(&self) -> &VersionManifest {
        &self.manifest
    }

    pub fn items(&self) -> &[PreparedContentItem] {
        &self.items
    }

    pub fn identity_digest(&self) -> [u8; 32] {
        self.manifest.identity_digest()
    }
}

pub struct VersioningPreflight<R, F, E, C> {
    repository: Arc<R>,
    storage: Arc<F>,
    executor: Arc<E>,
    clock: Arc<C>,
}

impl<R, F, E, C> VersioningPreflight<R, F, E, C>
where
    R: VersioningRepository + SemanticInspectionRepository,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    C: Clock,
{
    pub fn new(repository: Arc<R>, storage: Arc<F>, executor: Arc<E>, clock: Arc<C>) -> Self {
        Self {
            repository,
            storage,
            executor,
            clock,
        }
    }

    pub async fn prepare(
        &self,
        title: Title,
        items: Vec<VersioningItemInput>,
        profile: InspectionProfileVersion,
    ) -> Result<PreparedManifest, ApplicationError> {
        if items.is_empty() {
            return Err(ApplicationError::Validation(
                "Version requires at least one authoritative ContentItem".to_owned(),
            ));
        }
        let mut keys = BTreeSet::new();
        for item in &items {
            if item.original_filename.trim().is_empty()
                || item
                    .renditions
                    .iter()
                    .any(|rendition| rendition.original_filename.trim().is_empty())
                || !keys.insert((item.ordinal, item.logical_path.as_str().to_owned()))
            {
                return Err(ApplicationError::Validation(
                    "invalid or duplicate ContentItem key/filename".to_owned(),
                ));
            }
        }
        let ensure = EnsureSemanticInspection::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        );
        let mut prepared_items = Vec::with_capacity(items.len());
        let mut semantic_items = Vec::with_capacity(items.len());
        for item in items {
            let stored = self
                .storage
                .put_immutable(StoreFileRequest::new(
                    item.file_id,
                    item.content,
                    item.media_type,
                ))
                .await?;
            let file =
                FileObject::restore(item.file_id, stored.into_descriptor(), self.clock.now());
            self.repository.register_file_object(file.clone()).await?;
            let inspection = ensure.ensure(item.file_id, profile).await?;
            let response = inspection.response();
            let semantic = SemanticContentItem::new(
                item.logical_path.clone(),
                item.ordinal,
                format_id(response.detected_format),
                profile.as_str(),
                *response.semantic_fingerprint.digest(),
            )?;
            let mut renditions = Vec::with_capacity(item.renditions.len());
            for rendition in item.renditions {
                let stored = self
                    .storage
                    .put_immutable(StoreFileRequest::new(
                        rendition.file_id,
                        rendition.content,
                        rendition.media_type,
                    ))
                    .await?;
                let rendition_file = FileObject::restore(
                    rendition.file_id,
                    stored.into_descriptor(),
                    self.clock.now(),
                );
                self.repository
                    .register_file_object(rendition_file.clone())
                    .await?;
                renditions.push(PreparedRendition {
                    file: rendition_file,
                    original_filename: rendition.original_filename,
                });
            }
            semantic_items.push(semantic);
            prepared_items.push(PreparedContentItem {
                logical_path: item.logical_path,
                ordinal: item.ordinal,
                file,
                inspection,
                original_filename: item.original_filename,
                renditions,
            });
        }
        let manifest = VersionManifest::new(title, semantic_items)?;
        prepared_items.sort_by(|left, right| {
            (left.ordinal, left.logical_path.as_str())
                .cmp(&(right.ordinal, right.logical_path.as_str()))
        });
        Ok(PreparedManifest {
            manifest,
            items: prepared_items,
        })
    }

    pub async fn check_publish_quality(
        &self,
        prepared: &PreparedManifest,
    ) -> Result<(), ApplicationError> {
        check_publish_quality(prepared, self.storage.as_ref()).await
    }

    pub async fn inspect_existing(
        &self,
        snapshot: &AuthoritativeDocument,
    ) -> Result<PreparedManifest, ApplicationError> {
        if snapshot.requires_content_classification() || snapshot.content_items().is_empty() {
            return Err(ApplicationError::BusinessRule);
        }
        let ensure = EnsureSemanticInspection::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        );
        let mut prepared_items = Vec::with_capacity(snapshot.content_items().len());
        let mut semantic_items = Vec::with_capacity(snapshot.content_items().len());
        for item in snapshot.content_items() {
            let inspection = ensure
                .ensure(item.file().file_id(), InspectionProfileVersion::DsiV0)
                .await?;
            let response = inspection.response();
            semantic_items.push(SemanticContentItem::new(
                item.logical_path().clone(),
                item.ordinal(),
                format_id(response.detected_format),
                InspectionProfileVersion::DsiV0.as_str(),
                *response.semantic_fingerprint.digest(),
            )?);
            prepared_items.push(PreparedContentItem {
                logical_path: item.logical_path().clone(),
                ordinal: item.ordinal(),
                file: item.file().clone(),
                inspection,
                original_filename: item.original_filename().to_owned(),
                renditions: vec![],
            });
        }
        let manifest = VersionManifest::new(snapshot.version().title().clone(), semantic_items)?;
        prepared_items.sort_by(|left, right| {
            (left.ordinal, left.logical_path.as_str())
                .cmp(&(right.ordinal, right.logical_path.as_str()))
        });
        Ok(PreparedManifest {
            manifest,
            items: prepared_items,
        })
    }
}

const fn format_id(format: FormatId) -> &'static str {
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
