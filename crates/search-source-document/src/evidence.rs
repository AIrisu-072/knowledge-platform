//! Limited primary evidence for fields read directly from Document rows.

use std::collections::BTreeMap;

use search_application::ports::{
    BoxFuture, ClaimSelector, ClaimSelectorPort, EvidenceResolverPort, ResolvedAssertionEvidence,
};
use search_application::projection::ProjectionInput;
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::evidence::EvidenceRole;
use search_core::id::{ClaimId, ResourceId};
use search_core::predicate::TypedValue;
use search_core::profile::FacetState;
use search_core::projection::ProjectionGenerationKey;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::outbox::DocumentProjectionReader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentEvidenceField {
    Title,
    DocumentType,
    Category,
}

impl DocumentEvidenceField {
    const fn predicate(self) -> &'static str {
        match self {
            Self::Title => "document.title",
            Self::DocumentType => "document.document_type",
            Self::Category => "document.category",
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::DocumentType => "document_type",
            Self::Category => "category",
        }
    }

    const ALL: [Self; 3] = [Self::Title, Self::DocumentType, Self::Category];
}

pub(crate) fn locator(
    generation: ProjectionGenerationKey,
    resource: ResourceId,
    field: DocumentEvidenceField,
) -> String {
    format!(
        "document-field:v1:{}:{}:{}:{}",
        generation.source_id.as_uuid(),
        generation.generation_id.as_uuid(),
        resource.as_uuid(),
        field.key(),
    )
}

pub(crate) fn stored_locator(
    source: search_core::id::SourceId,
    resource: ResourceId,
    field: DocumentEvidenceField,
) -> String {
    format!(
        "document-field:stored:v1:{}:{}:{}",
        source.as_uuid(),
        resource.as_uuid(),
        field.key(),
    )
}

pub(crate) fn expose_generation_locators(
    assertion: &mut Assertion,
    generation: ProjectionGenerationKey,
    resource: ResourceId,
) {
    for field in DocumentEvidenceField::ALL {
        let stored = stored_locator(generation.source_id, resource, field);
        for reference in &mut assertion.evidence_refs {
            if *reference == stored {
                *reference = locator(generation, resource, field);
            }
        }
    }
}

pub(crate) fn append_authoritative_assertions(
    input: &mut ProjectionInput,
    document_id: document_domain::DocumentId,
    observed_at: OffsetDateTime,
) {
    let resource = input.resource.identity.resource_id;
    let fields = [
        (DocumentEvidenceField::Title, input.title.clone()),
        (
            DocumentEvidenceField::DocumentType,
            field_value(input, "document_type"),
        ),
        (
            DocumentEvidenceField::Category,
            field_value(input, "category"),
        ),
    ];
    for (field, value) in fields {
        let Some(value) = value else { continue };
        let mut assertion = Assertion::new(
            format!("document-version:{}", resource.as_uuid()),
            field.predicate(),
            TypedValue::String(value),
            input.source.source_id.as_uuid().to_string(),
            AssertionOrigin::Authoritative,
            format!("document:{}", document_id.as_uuid()),
            observed_at,
        );
        assertion
            .evidence_refs
            .push(stored_locator(input.source.source_id, resource, field));
        input.assertions.push(assertion);
    }
}

fn field_value(input: &ProjectionInput, key: &str) -> Option<String> {
    match input.typed_facets.get(key) {
        Some(FacetState::Known(TypedValue::String(value))) => Some(value.clone()),
        _ => None,
    }
}

/// Trusted query preparation binds ClaimId to a typed, Source-owned field.
/// The selector is exposed only when the pinned projection contains that field.
pub struct DocumentEvidenceCatalog {
    reader: DocumentProjectionReader,
    bindings: BTreeMap<ClaimId, (ResourceId, DocumentEvidenceField, Option<String>)>,
}

impl DocumentEvidenceCatalog {
    pub fn new(reader: DocumentProjectionReader) -> Self {
        Self {
            reader,
            bindings: BTreeMap::new(),
        }
    }

    pub fn bind_claim(
        &mut self,
        claim_id: ClaimId,
        resource: ResourceId,
        field: DocumentEvidenceField,
        expected_value: Option<String>,
    ) {
        self.bindings
            .insert(claim_id, (resource, field, expected_value));
    }
}

impl ClaimSelectorPort for DocumentEvidenceCatalog {
    fn selector_for<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            let Some((resource, field, expected)) = self.bindings.get(&claim_id) else {
                return Ok(None);
            };
            let Some(projection) = self.reader.resource_at(generation, *resource).await? else {
                return Ok(None);
            };
            if !projection.structured.assertions.iter().any(|assertion| {
                assertion.predicate == field.predicate()
                    && assertion.origin == AssertionOrigin::Authoritative
                    && assertion.evidence_refs.contains(&stored_locator(
                        generation.source_id,
                        *resource,
                        *field,
                    ))
            }) {
                return Ok(None);
            }
            Ok(Some(ClaimSelector {
                claim_id,
                subject_ref: format!("document-version:{}", resource.as_uuid()),
                predicate: field.predicate().into(),
                expected_value: expected.clone().map(TypedValue::String),
            }))
        })
    }
}

impl EvidenceResolverPort for DocumentEvidenceCatalog {
    fn resolve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        resource_ref: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move {
            let Some(field) = DocumentEvidenceField::ALL
                .into_iter()
                .find(|field| locator(generation, resource_ref, *field) == evidence_ref)
            else {
                return Ok(None);
            };
            let Some(projection) = self.reader.resource_at(generation, resource_ref).await? else {
                return Ok(None);
            };
            let Some(assertion) = projection.structured.assertions.iter().find(|assertion| {
                assertion.predicate == field.predicate()
                    && assertion.origin == AssertionOrigin::Authoritative
                    && assertion.source_ref == generation.source_id.as_uuid().to_string()
                    && assertion.evidence_refs.iter().any(|item| {
                        item == &stored_locator(generation.source_id, resource_ref, field)
                    })
            }) else {
                return Ok(None);
            };
            let TypedValue::String(value) = &assertion.value else {
                return Ok(None);
            };
            let mut digest = Sha256::new();
            digest.update(value.as_bytes());
            let content_digest = digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            Ok(Some(ResolvedAssertionEvidence {
                generation,
                source_id: generation.source_id,
                resource_id: resource_ref,
                evidence_ref: evidence_ref.into(),
                upstream_origin: format!(
                    "{}:version:{}:field:{}",
                    assertion.authority_scope,
                    resource_ref.as_uuid(),
                    field.key()
                ),
                role: EvidenceRole::Primary,
                citation_chain: vec![],
                content_digest: Some(format!("sha256:{content_digest}")),
                is_summary: false,
            }))
        })
    }
}
