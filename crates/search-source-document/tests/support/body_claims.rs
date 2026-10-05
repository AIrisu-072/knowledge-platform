//! Shared Source-side fixtures for exact positive and negative body evidence.
#![allow(dead_code, unused_imports)]

pub use std::collections::VecDeque;

pub use super::body_index::*;
pub use search_application::body_ports::{
    BodyCoverageGapPort, CONTAINS_EXACT_PREDICATE, ExactTextEvidencePort, ExactTextSelector,
    KnowledgeUnitHitRef,
};
pub use search_application::ports::{
    AccessDecision, CurrentAccessEvaluatorPort, LexicalQuery, LexicalRetrieverPort,
    assemble_verified_unit_text_claim,
};
pub use search_core::assertion::AssertionOrigin;
pub use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
pub use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceRole};
pub use search_core::id::{
    ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId,
};
pub use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
pub use search_core::knowledge_unit::{ExtractionProfileId, FormatId, TextSpan};
pub use search_core::projection::ProjectionGenerationKey;
pub use search_core::temporal::TemporalEvaluationContext;
pub use search_source_document::{
    BodyItemExtractor, DocumentBodyCoverageGaps, DocumentExactTextEvidenceCatalog,
};

pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

pub fn claim() -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(500))
}

pub fn parent() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(20))
}

pub fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(400).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(501)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find body".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![],
            required_claims: vec![claim()],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![claim()]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(502)),
            now,
            now,
            "Asia/Tokyo",
        ),
        access_context: "trusted-session".into(),
    }
}

/// Returns queued decisions first, then the default.
pub struct ScriptedAccess {
    pub queued: Mutex<VecDeque<AccessDecision>>,
    pub default: AccessDecision,
}

pub fn access(default: AccessDecision, queued: &[AccessDecision]) -> Arc<ScriptedAccess> {
    Arc::new(ScriptedAccess {
        queued: Mutex::new(queued.iter().copied().collect()),
        default,
    })
}

impl CurrentAccessEvaluatorPort for ScriptedAccess {
    fn evaluate<'a>(&'a self, _: ResourceId, _: &'a str) -> BoxFuture<'a, AccessDecision> {
        let decision = self
            .queued
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(self.default);
        Box::pin(async move { Ok(decision) })
    }
}

/// A DOCX whose referenced header is a located omission: Completed + Partial.
pub fn partial_docx(text: &str) -> Vec<u8> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
    let package = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr></w:body></w:document>"#
    );
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    let header = r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>ヘッダ</w:t></w:r></w:p></w:hdr>"#;
    super::body_support::zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", package.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
    ])
}

pub struct Published {
    pub harness: Harness,
    pub key: ProjectionGenerationKey,
    pub items: Vec<AuthoritativeItemBinding>,
}

/// Part 0 holds 東京の本文 (text or Partial DOCX), part 1 a Supported text,
/// part 2 an Unsupported image.
pub async fn publish(partial: bool) -> Published {
    let first = if partial {
        (partial_docx("東京の本文"), DOCX)
    } else {
        ("東京の本文\n".as_bytes().to_vec(), "text/plain")
    };
    publish_items(vec![
        first,
        ("大阪の本文\n".as_bytes().to_vec(), "text/plain"),
        (b"\x89PNG\r\n\x1a\n".to_vec(), "image/png"),
    ])
    .await
}

/// Publishes one current Version whose parts are the given raw items in order.
pub async fn publish_items(raw: Vec<(Vec<u8>, &str)>) -> Published {
    let storage = KeyedStorage::default();
    let mut items = Vec::new();
    for (index, (bytes, media)) in raw.iter().enumerate() {
        let key = format!("objects/{index}");
        storage.put(&key, bytes);
        let ordinal = u32::try_from(index).unwrap();
        items.push(binding(
            &key,
            bytes,
            media,
            ordinal,
            100 * (index as u128 + 1),
        ));
    }
    let harness = harness(snapshot("s1", items.clone()), storage, true);
    let key = published(&harness, 1).await;
    Published {
        harness,
        key,
        items,
    }
}

impl Published {
    pub async fn hit(&self, text: &str) -> KnowledgeUnitHitRef {
        let batch = self
            .harness
            .runtime
            .lexical_reader()
            .retrieve_body(self.key, &request(), &LexicalQuery::body_only(text, 10))
            .await
            .unwrap();
        batch.hits[0].unit_hit.clone().expect("literal Unit hit")
    }

    pub fn catalog(&self, access: Arc<ScriptedAccess>) -> DocumentExactTextEvidenceCatalog {
        self.catalog_with(access, &[selector("東京の本文")])
    }

    pub fn catalog_with(
        &self,
        access: Arc<ScriptedAccess>,
        selectors: &[ExactTextSelector],
    ) -> DocumentExactTextEvidenceCatalog {
        let extractor: Arc<dyn BodyItemExtractor> = Arc::new(DocumentBodyExtractor::new(
            source_id(),
            self.harness.storage.clone(),
            InProcessExtractor::new(Mode::Honest),
            registry(),
        ));
        let mut catalog = DocumentExactTextEvidenceCatalog::new(
            source_id(),
            self.harness.runtime.clone(),
            Arc::new(self.harness.reader.clone()),
            access,
            extractor,
        );
        for selector in selectors {
            catalog.register(selector.clone()).unwrap();
        }
        catalog
    }
}

pub fn selector(text: &str) -> ExactTextSelector {
    ExactTextSelector {
        claim_id: claim(),
        parent_resource: parent(),
        predicate: CONTAINS_EXACT_PREDICATE.into(),
        expected_exact_text: text.into(),
    }
}

pub fn verified_some(result: Result<Option<impl Sized>, search_application::SearchError>) -> bool {
    matches!(result, Ok(Some(_)))
}
