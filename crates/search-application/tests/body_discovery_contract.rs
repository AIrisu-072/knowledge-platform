//! P1-A02 request-scoped `BodyRequired` through the shared Discovery loop.

#[path = "support/body_discovery.rs"]
mod body_discovery;

use body_discovery::*;

#[tokio::test]
async fn body_scope_runs_only_lexical_with_the_request_query() {
    let fixture = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(10), "lexical"),
            unit_hit: Some(unit_hit(rid(10))),
        }],
        true,
    ));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), body("本文語"))
        .await
        .unwrap();
    assert_eq!(fixture.calls(), vec!["lexical-body".to_owned()]);
    let queries = fixture.body_queries.lock().unwrap().clone();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].text, "本文語");
    assert_eq!(queries[0].field_scope, LexicalFieldScope::BodyOnly);
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    let qualified: Vec<_> = result
        .qualified_resources
        .iter()
        .map(|resource| resource.resource_ref)
        .collect();
    assert_eq!(qualified, vec![rid(10)]);
    // The Unit reference survives federation into the qualification records.
    assert!(
        result
            .qualification_trace
            .contains(&"content_scope:body_required:unit_hits:1".to_owned())
    );
}

#[tokio::test]
async fn title_or_token_only_hits_never_qualify_body_scope() {
    // Normal scope is satisfied by the structured title/metadata path.
    let fixture = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(20), "lexical"),
            unit_hit: None,
        }],
        true,
    ));
    let normal = fixture.service(config()).discover(request()).await.unwrap();
    assert_eq!(normal.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    assert!(!normal.qualified_resources.is_empty());

    // The same Resource reached only by a token hit without a literal span is not a body match.
    let fixture = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(20), "lexical"),
            unit_hit: None,
        }],
        true,
    ));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), body("本文語"))
        .await
        .unwrap();
    assert!(
        !fixture
            .calls()
            .iter()
            .any(|call| call == "structured" || call == "directory")
    );
    assert!(result.qualified_resources.is_empty());
    assert!(result.evidence_set.is_empty());
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(gap_codes(&result).contains(&"document.body.match_unproven".to_owned()));
}

#[tokio::test]
async fn missing_body_port_bundle_or_foreign_unit_is_a_blocking_gap() {
    let refused = Fixture::new(BodyPort::Refuse);
    let result = refused
        .service(config())
        .discover_with_content_scope(request(), body("本文語"))
        .await
        .unwrap();
    assert!(result.qualified_resources.is_empty());
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(gap_codes(&result).contains(&"document.body.retrieval_unavailable".to_owned()));

    // A Unit whose parent differs from the candidate Resource is rejected, not trusted.
    let foreign = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(10), "lexical"),
            unit_hit: Some(unit_hit(rid(11))),
        }],
        true,
    ));
    let result = foreign
        .service(config())
        .discover_with_content_scope(request(), body("本文語"))
        .await
        .unwrap();
    assert!(result.qualified_resources.is_empty());
    assert!(gap_codes(&result).contains(&"document.body.retrieval_unavailable".to_owned()));

    // Without a Lexical adapter there is no executable body action at all.
    let mut no_lexical = config();
    no_lexical.retriever_support.lexical = false;
    let fixture = Fixture::new(BodyPort::Refuse);
    let result = fixture
        .service(no_lexical)
        .discover_with_content_scope(request(), body("本文語"))
        .await
        .unwrap();
    assert!(fixture.calls().is_empty());
    assert!(result.qualified_resources.is_empty());
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
}

#[tokio::test]
async fn malformed_or_swapped_body_spec_is_rejected() {
    let fixture = Fixture::new(BodyPort::Refuse);
    let service = fixture.service(config());
    for query in [
        LexicalQuery::new("本文語", 10),
        LexicalQuery::body_only("  ", 10),
        LexicalQuery::body_only("本文語", 0),
        LexicalQuery::body_only("本文語", 257),
    ] {
        let scope = DiscoveryScope::BodyRequired(BodySearchSpec {
            query,
            exact_text_claim: None,
        });
        assert!(matches!(
            service.discover_with_content_scope(request(), scope).await,
            Err(SearchError::InvalidRequest(_))
        ));
    }
    assert!(fixture.calls().is_empty());
}
