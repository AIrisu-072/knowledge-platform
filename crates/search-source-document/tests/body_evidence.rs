//! P1-A03 Source-owned exact positive evidence: pinned manifest Unit, current
//! Live Version and Read, and the same raw bytes rebuilding the same span.

#[path = "support/body_claims.rs"]
mod body_claims;
#[path = "support/body_index.rs"]
mod body_index;
#[path = "support/body.rs"]
mod body_support;

use body_claims::*;

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
        .put("objects/0", "東京の別文\n".as_bytes());
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
