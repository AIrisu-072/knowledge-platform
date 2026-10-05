//! P1-A03 Source-owned exact positive evidence: pinned manifest Unit, current
//! Live Version and Read, and the same raw bytes rebuilding the same span.

#[path = "support/body_index.rs"]
mod body_index;
#[path = "support/body.rs"]
mod body_support;

use std::collections::VecDeque;

use body_index::*;
use search_application::body_ports::{
    BodyCoverageGapPort, CONTAINS_EXACT_PREDICATE, ExactTextEvidencePort, ExactTextSelector,
    KnowledgeUnitHitRef,
};
use search_application::ports::{
    AccessDecision, CurrentAccessEvaluatorPort, LexicalQuery, LexicalRetrieverPort,
    assemble_verified_unit_text_claim,
};
use search_core::assertion::AssertionOrigin;
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceRole};
use search_core::id::{ClaimId, DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::knowledge_unit::{ExtractionProfileId, FormatId, TextSpan};
use search_core::projection::ProjectionGenerationKey;
use search_core::temporal::TemporalEvaluationContext;
use search_source_document::{
    BodyItemExtractor, DocumentBodyCoverageGaps, DocumentExactTextEvidenceCatalog,
};

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn claim() -> ClaimId {
    ClaimId::from_uuid(Uuid::from_u128(500))
}

fn parent() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(20))
}

fn request() -> DiscoveryRequest {
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
struct ScriptedAccess {
    queued: Mutex<VecDeque<AccessDecision>>,
    default: AccessDecision,
}

fn access(default: AccessDecision, queued: &[AccessDecision]) -> Arc<ScriptedAccess> {
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
fn partial_docx(text: &str) -> Vec<u8> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
    let package = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr></w:body></w:document>"#
    );
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    let header = r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>ヘッダ</w:t></w:r></w:p></w:hdr>"#;
    body_support::zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", package.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
    ])
}

struct Published {
    harness: Harness,
    key: ProjectionGenerationKey,
    items: Vec<AuthoritativeItemBinding>,
}

/// Part 0 holds 東京の本文 (text or Partial DOCX), part 1 a Supported text,
/// part 2 an Unsupported image.
async fn publish(partial: bool) -> Published {
    let storage = KeyedStorage::default();
    let (first, media) = if partial {
        (partial_docx("東京の本文"), DOCX)
    } else {
        ("東京の本文\n".as_bytes().to_vec(), "text/plain")
    };
    let second = "大阪の本文\n".as_bytes().to_vec();
    let third = b"\x89PNG\r\n\x1a\n".to_vec();
    storage.put("objects/a", &first);
    storage.put("objects/b", &second);
    storage.put("objects/c", &third);
    let items = vec![
        binding("objects/a", &first, media, 0, 100),
        binding("objects/b", &second, "text/plain", 1, 200),
        binding("objects/c", &third, "image/png", 2, 300),
    ];
    let harness = harness(snapshot("s1", items.clone()), storage, true);
    let key = published(&harness, 1).await;
    Published {
        harness,
        key,
        items,
    }
}

impl Published {
    async fn hit(&self, text: &str) -> KnowledgeUnitHitRef {
        let batch = self
            .harness
            .runtime
            .lexical_reader()
            .retrieve_body(self.key, &request(), &LexicalQuery::body_only(text, 10))
            .await
            .unwrap();
        batch.hits[0].unit_hit.clone().expect("literal Unit hit")
    }

    fn catalog(&self, access: Arc<ScriptedAccess>) -> DocumentExactTextEvidenceCatalog {
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
        catalog.register(selector("東京の本文")).unwrap();
        catalog
    }
}

fn selector(text: &str) -> ExactTextSelector {
    ExactTextSelector {
        claim_id: claim(),
        parent_resource: parent(),
        predicate: CONTAINS_EXACT_PREDICATE.into(),
        expected_exact_text: text.into(),
    }
}

fn verified_some(result: Result<Option<impl Sized>, search_application::SearchError>) -> bool {
    matches!(result, Ok(Some(_)))
}

#[tokio::test]
async fn source_reread_verifies_the_unit_span_for_the_bound_claim() {
    let published = publish(false).await;
    let hit = published.hit("東京の本文").await;
    let catalog = published.catalog(access(AccessDecision::Allowed, &[]));
    let bound = selector("東京の本文");
    assert_eq!(
        catalog.selector_for(published.key, claim()).await.unwrap(),
        Some(bound.clone())
    );
    let other_source = ProjectionGenerationKey {
        source_id: search_core::id::SourceId::from_uuid(Uuid::from_u128(999)),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(1)),
    };
    assert_eq!(
        catalog.selector_for(other_source, claim()).await.unwrap(),
        None
    );

    let verified = catalog
        .resolve_hit(&request(), &hit, &bound)
        .await
        .unwrap()
        .expect("verified exact span");
    assert_eq!(verified.assertion.origin, AssertionOrigin::Extracted);
    assert_eq!(
        verified.assertion.subject_ref,
        format!("document-version:{}", parent().as_uuid())
    );
    assert_eq!(verified.resolved.role, EvidenceRole::Primary);
    assert!(!verified.resolved.is_summary);
    let digest: String = Sha256::digest("東京の本文".as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        verified.resolved.content_digest,
        Some(format!("sha256:{digest}"))
    );
    assert_eq!(verified.matched_span, hit.span);
    assert_eq!(
        assemble_verified_unit_text_claim(claim(), &bound, &verified).state,
        ClaimState::Supported
    );
}

#[tokio::test]
async fn forged_or_mismatched_hits_never_verify() {
    let published = publish(false).await;
    let hit = published.hit("東京の本文").await;
    let other = published.hit("大阪の本文").await;
    let catalog = published.catalog(access(AccessDecision::Allowed, &[]));
    let bound = selector("東京の本文");
    let forged: Vec<KnowledgeUnitHitRef> = vec![
        KnowledgeUnitHitRef {
            unit_id: other.unit_id,
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            part: other.part.clone(),
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            opaque_locator: "00".into(),
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            span: TextSpan {
                start_byte: 3,
                end_byte: hit.span.end_byte,
            },
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            raw: RawBinding {
                sha256: [0; 32],
                ..hit.raw.clone()
            },
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            profile: ExtractionProfileId::for_definition(&body_support::definition(FormatId::Csv))
                .unwrap(),
            ..hit.clone()
        },
        KnowledgeUnitHitRef {
            parent_resource: ResourceId::from_uuid(Uuid::from_u128(21)),
            ..hit.clone()
        },
    ];
    for (index, forged) in forged.iter().enumerate() {
        let result = catalog.resolve_hit(&request(), forged, &bound).await;
        assert!(!verified_some(result), "forged hit {index}");
    }
    // Selectors must be the registered one, for a required Claim and this parent.
    let wrong_text = selector("大阪の本文");
    let wrong_claim = ExactTextSelector {
        claim_id: ClaimId::from_uuid(Uuid::from_u128(77)),
        ..bound.clone()
    };
    let wrong_parent = ExactTextSelector {
        parent_resource: ResourceId::from_uuid(Uuid::from_u128(21)),
        ..bound.clone()
    };
    for selector in [wrong_text, wrong_claim, wrong_parent] {
        let result = catalog.resolve_hit(&request(), &hit, &selector).await;
        assert!(!verified_some(result));
    }
    let mut registry = published.catalog(access(AccessDecision::Allowed, &[]));
    assert!(
        registry
            .register(ExactTextSelector {
                predicate: "document.title".into(),
                ..bound.clone()
            })
            .is_err()
    );
    assert!(registry.register(selector("東京\r\nの本文")).is_err());
}

#[tokio::test]
async fn read_or_source_change_before_disclosure_is_unknown() {
    let published = publish(false).await;
    let hit = published.hit("東京の本文").await;
    let bound = selector("東京の本文");
    for scripted in [
        access(AccessDecision::Denied, &[]),
        access(AccessDecision::Unknown, &[]),
        // Allowed at first, revoked before disclosure.
        access(AccessDecision::Denied, &[AccessDecision::Allowed]),
    ] {
        let catalog = published.catalog(scripted);
        let result = catalog.resolve_hit(&request(), &hit, &bound).await;
        assert!(!verified_some(result));
    }

    // The Version is no longer the current Live Version.
    let catalog = published.catalog(access(AccessDecision::Allowed, &[]));
    let mut superseded = snapshot("s2", published.items.clone());
    superseded.live[0].snapshot.current_version_id =
        Some(DocumentVersionId::from_uuid(Uuid::from_u128(21)));
    published.harness.reader.replace(vec![superseded]);
    assert!(!verified_some(
        catalog.resolve_hit(&request(), &hit, &bound).await
    ));
    published
        .harness
        .reader
        .replace(vec![snapshot("s1", published.items.clone())]);
    assert!(verified_some(
        catalog.resolve_hit(&request(), &hit, &bound).await
    ));

    // The stored raw bytes changed after publication.
    published
        .harness
        .storage
        .put("objects/a", "東京の別文\n".as_bytes());
    assert!(!verified_some(
        catalog.resolve_hit(&request(), &hit, &bound).await
    ));
}

#[tokio::test]
async fn partial_item_verifies_positive_and_reports_filtered_coverage_gaps() {
    let published = publish(true).await;
    let hit = published.hit("東京の本文").await;
    let catalog = published.catalog(access(AccessDecision::Allowed, &[]));
    assert!(verified_some(
        catalog
            .resolve_hit(&request(), &hit, &selector("東京の本文"))
            .await
    ));

    let gaps = |decision, queued: &[AccessDecision]| {
        DocumentBodyCoverageGaps::new(published.harness.runtime.clone(), access(decision, queued))
    };
    let codes = |result: Vec<search_core::discovery::InformationGap>| {
        result
            .into_iter()
            .map(|gap| (gap.required_fact, gap.blocking))
            .collect::<Vec<_>>()
    };
    let visible = codes(
        gaps(AccessDecision::Allowed, &[])
            .coverage_gaps(&request(), published.key)
            .await
            .unwrap(),
    );
    assert_eq!(
        visible,
        vec![
            (
                format!("document.body.coverage:{}:0:partial", parent().as_uuid()),
                true
            ),
            (
                format!(
                    "document.body.coverage:{}:2:unsupported",
                    parent().as_uuid()
                ),
                true
            ),
        ]
    );
    // Denied leaves nothing; Unknown leaves one gap without IDs or counts.
    assert!(
        gaps(AccessDecision::Denied, &[])
            .coverage_gaps(&request(), published.key)
            .await
            .unwrap()
            .is_empty()
    );
    for scripted in [
        gaps(AccessDecision::Unknown, &[]),
        // Allowed at first, undecidable at the pre-disclosure recheck.
        gaps(AccessDecision::Unknown, &[AccessDecision::Allowed]),
    ] {
        assert_eq!(
            codes(
                scripted
                    .coverage_gaps(&request(), published.key)
                    .await
                    .unwrap()
            ),
            vec![("document.body.coverage_undetermined".to_owned(), true)]
        );
    }
    let unpublished = ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(4242)),
    };
    assert_eq!(
        codes(
            gaps(AccessDecision::Allowed, &[])
                .coverage_gaps(&request(), unpublished)
                .await
                .unwrap()
        ),
        vec![("document.body.bundle_unavailable".to_owned(), true)]
    );
}
