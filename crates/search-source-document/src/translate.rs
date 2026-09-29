use std::collections::BTreeMap;

use document_domain::LifecycleState;
use search_application::projection::ProjectionInput;
use search_core::id::{ResourceId, ResourceVersionId};
use search_core::predicate::TypedValue;
use search_core::profile::{DiscoveryLens, DiscoveryProfile, FacetState};
use search_core::resource::{DiscoverableResource, ResourceBody, ResourceIdentity, ResourceKind};
use search_core::source::DiscoverableSource;
use search_tantivy::{LexicalBuildInput, LexicalDocument};

use crate::model::{DocumentSourceSnapshot, DsiEvidenceRefs, PublicationEndRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentVisibilityClass {
    CurrentPublished,
    HistoricalPublished,
    Withdrawn,
    PublicationEnded,
    WorkingAuthoring,
}

impl DocumentVisibilityClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CurrentPublished => "current_published",
            Self::HistoricalPublished => "historical_published",
            Self::Withdrawn => "withdrawn",
            Self::PublicationEnded => "publication_ended",
            Self::WorkingAuthoring => "working_authoring",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocumentIndexInputs {
    pub projection: ProjectionInput,
    /// One version's lexical fields, available for same-snapshot generation assembly.
    pub lexical_document: LexicalDocument,
    pub lexical: LexicalBuildInput,
    pub publication_end: Option<PublicationEndRecord>,
    pub dsi: Option<DsiEvidenceRefs>,
}

/// The route is explicit: historical and authoring inputs cannot be mistaken for Live.
#[derive(Debug, Clone)]
pub enum DocumentSourceTranslation {
    Live(DocumentIndexInputs),
    Historical {
        visibility: DocumentVisibilityClass,
        inputs: DocumentIndexInputs,
    },
    Authoring {
        visibility: DocumentVisibilityClass,
        inputs: DocumentIndexInputs,
    },
}

impl DocumentSourceTranslation {
    pub fn visibility(&self) -> DocumentVisibilityClass {
        match self {
            Self::Live(_) => DocumentVisibilityClass::CurrentPublished,
            Self::Historical { visibility, .. } | Self::Authoring { visibility, .. } => *visibility,
        }
    }

    pub fn live_inputs(&self) -> Option<&DocumentIndexInputs> {
        match self {
            Self::Live(inputs) => Some(inputs),
            _ => None,
        }
    }

    pub fn historical_inputs(&self) -> Option<&DocumentIndexInputs> {
        match self {
            Self::Historical { inputs, .. } => Some(inputs),
            _ => None,
        }
    }

    pub fn authoring_inputs(&self) -> Option<&DocumentIndexInputs> {
        match self {
            Self::Authoring { inputs, .. } => Some(inputs),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslationError {
    #[error("publication ended but the Document still has a current Version")]
    PublicationEndedWithCurrent,
    #[error("current DocumentVersion is not PUBLISHED")]
    CurrentVersionNotPublished,
    #[error("PUBLISHED DocumentVersion has no published_at")]
    MissingPublishedAt,
    #[error("WITHDRAWN DocumentVersion has no withdrawn_at")]
    MissingWithdrawnAt,
    #[error("DocumentVersion timestamps contradict its lifecycle")]
    InconsistentLifecycle,
    #[error("Document Source snapshot identifier is empty")]
    MissingSourceSnapshot,
    #[error("Document Source or Lens does not describe Knowledge in this Source")]
    InvalidSourceConfiguration,
    #[error("Document Lens requests a lexical field outside title and permitted metadata")]
    UnsupportedLexicalField,
}

pub struct DocumentSourceTranslator {
    source: DiscoverableSource,
    lens: DiscoveryLens,
    projection_schema_version: String,
    semantic_registry_version: String,
}

impl DocumentSourceTranslator {
    pub fn new(
        source: DiscoverableSource,
        lens: DiscoveryLens,
        projection_schema_version: impl Into<String>,
        semantic_registry_version: impl Into<String>,
    ) -> Self {
        Self {
            source,
            lens,
            projection_schema_version: projection_schema_version.into(),
            semantic_registry_version: semantic_registry_version.into(),
        }
    }

    pub fn translate(
        &self,
        snapshot: DocumentSourceSnapshot,
    ) -> Result<DocumentSourceTranslation, TranslationError> {
        if snapshot.source_snapshot.trim().is_empty() {
            return Err(TranslationError::MissingSourceSnapshot);
        }
        if self.lens.resource_type != ResourceKind::Knowledge
            || self.lens.source_scope != Some(self.source.source_id)
            || !self
                .source
                .resource_types
                .contains(&ResourceKind::Knowledge)
        {
            return Err(TranslationError::InvalidSourceConfiguration);
        }
        if self
            .lens
            .searchable_fields
            .iter()
            .any(|field| !matches!(field.as_str(), "title" | "permitted_metadata"))
        {
            return Err(TranslationError::UnsupportedLexicalField);
        }
        if snapshot.publication_end.is_some() && snapshot.current_version_id.is_some() {
            return Err(TranslationError::PublicationEndedWithCurrent);
        }
        if snapshot.current_version_id == Some(snapshot.document_version_id)
            && snapshot.lifecycle_state != LifecycleState::Published
        {
            return Err(TranslationError::CurrentVersionNotPublished);
        }
        match snapshot.lifecycle_state {
            LifecycleState::Published if snapshot.published_at.is_none() => {
                return Err(TranslationError::MissingPublishedAt);
            }
            LifecycleState::Withdrawn if snapshot.withdrawn_at.is_none() => {
                return Err(TranslationError::MissingWithdrawnAt);
            }
            LifecycleState::Published if snapshot.withdrawn_at.is_some() => {
                return Err(TranslationError::InconsistentLifecycle);
            }
            LifecycleState::Working
                if snapshot.published_at.is_some() || snapshot.withdrawn_at.is_some() =>
            {
                return Err(TranslationError::InconsistentLifecycle);
            }
            _ => {}
        }

        let visibility = if snapshot.publication_end.is_some() {
            DocumentVisibilityClass::PublicationEnded
        } else {
            match snapshot.lifecycle_state {
                LifecycleState::Published
                    if snapshot.current_version_id == Some(snapshot.document_version_id) =>
                {
                    DocumentVisibilityClass::CurrentPublished
                }
                LifecycleState::Published => DocumentVisibilityClass::HistoricalPublished,
                LifecycleState::Withdrawn => DocumentVisibilityClass::Withdrawn,
                LifecycleState::Working => DocumentVisibilityClass::WorkingAuthoring,
            }
        };

        let title = snapshot.title.as_str().to_owned();
        let mut profile = DiscoveryProfile::new(&title);
        let mut typed_facets = BTreeMap::new();
        if let Some(value) = snapshot.metadata.document_type.as_ref() {
            add_metadata_facet(&mut profile, &mut typed_facets, "document_type", value);
        }
        if let Some(value) = snapshot.metadata.category.as_ref() {
            add_metadata_facet(&mut profile, &mut typed_facets, "category", value);
        }
        typed_facets.insert(
            "document.folder_id".into(),
            FacetState::Known(TypedValue::String(snapshot.folder_id.as_uuid().to_string())),
        );
        typed_facets.insert(
            "document.lifecycle".into(),
            FacetState::Known(TypedValue::String(
                match snapshot.lifecycle_state {
                    LifecycleState::Working => "WORKING",
                    LifecycleState::Published => "PUBLISHED",
                    LifecycleState::Withdrawn => "WITHDRAWN",
                }
                .into(),
            )),
        );
        typed_facets.insert(
            "document.visibility".into(),
            FacetState::Known(TypedValue::String(visibility.as_str().into())),
        );
        if let Some(record) = snapshot.publication_end.as_ref() {
            typed_facets.insert(
                "document.publication_ended_at".into(),
                FacetState::Known(TypedValue::DateTime(record.ended_at)),
            );
        }

        let resource_ref = ResourceId::from_uuid(snapshot.document_version_id.as_uuid());
        let mut identity =
            ResourceIdentity::new(resource_ref, ResourceKind::Knowledge, self.source.source_id);
        identity.resource_version = Some(ResourceVersionId::from_uuid(
            snapshot.document_version_id.as_uuid(),
        ));
        identity.source_native_id = Some(snapshot.document_id.as_uuid().to_string());
        identity.valid_from = snapshot.effective_from;
        identity.valid_to = snapshot.effective_to;
        identity.access_scope = snapshot.access.access_scope;
        let mut resource = DiscoverableResource::new(identity, ResourceBody::Knowledge, profile);
        resource.temporal_profile.effective_from = snapshot.effective_from;
        resource.temporal_profile.effective_to = snapshot.effective_to;

        let source_snapshot = snapshot.source_snapshot;
        let lexical_document = LexicalDocument {
            resource_ref,
            kind: ResourceKind::Knowledge,
            canonical_name: title.clone(),
            title: Some(title.clone()),
            aliases: Vec::new(),
            high_signal_text: self
                .lens
                .searchable_fields
                .iter()
                .any(|field| field == "permitted_metadata")
                .then(|| permitted_lexical_metadata(&snapshot.metadata))
                .flatten(),
            body: None,
            locator: None,
        };
        let lexical = LexicalBuildInput::new(
            self.source.source_id,
            source_snapshot.clone(),
            self.projection_schema_version.clone(),
            self.lens.lens_version,
            vec![lexical_document.clone()],
        );
        let inputs = DocumentIndexInputs {
            projection: ProjectionInput {
                resource,
                source: self.source.clone(),
                lens: self.lens.clone(),
                source_snapshot,
                projection_schema_version: self.projection_schema_version.clone(),
                semantic_registry_version: self.semantic_registry_version.clone(),
                title: Some(title),
                typed_facets,
                assertions: Vec::new(),
                authority_resolutions: BTreeMap::new(),
                relations: Vec::new(),
            },
            lexical_document,
            lexical,
            publication_end: snapshot.publication_end,
            dsi: snapshot.dsi,
        };
        Ok(match snapshot.lifecycle_state {
            LifecycleState::Working => DocumentSourceTranslation::Authoring { visibility, inputs },
            _ if visibility == DocumentVisibilityClass::CurrentPublished => {
                DocumentSourceTranslation::Live(inputs)
            }
            _ => DocumentSourceTranslation::Historical { visibility, inputs },
        })
    }
}

fn add_metadata_facet(
    profile: &mut DiscoveryProfile,
    typed_facets: &mut BTreeMap<String, FacetState<TypedValue>>,
    name: &str,
    value: &str,
) {
    profile
        .high_signal_facets
        .insert(name.into(), FacetState::Known(value.into()));
    typed_facets.insert(
        name.into(),
        FacetState::Known(TypedValue::String(value.into())),
    );
}

fn permitted_lexical_metadata(
    metadata: &crate::model::PermittedDocumentMetadata,
) -> Option<String> {
    let values: Vec<_> = [
        metadata.document_type.as_deref(),
        metadata.category.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.trim().is_empty())
    .collect();
    (!values.is_empty()).then(|| values.join(" "))
}
