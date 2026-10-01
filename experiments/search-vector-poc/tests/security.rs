use search_application::ports::AccessDecision;
use search_core::id::{ProjectionGenerationId, ResourceId};
use search_core::source::RetentionMode;
use search_vector_poc::corpus::{Corpus, SEED};
use search_vector_poc::run::{ACTOR, Arm, Harness, pinned, run_arm};
use search_vector_poc::validate::{MockVectorHit, validate_mock_vector_hit};
use uuid::Uuid;

#[tokio::test]
async fn actual_lexical_graph_federation_preserves_s1_and_graph_only_gold() {
    let harness = Harness::build(Corpus::synthetic(32, SEED).unwrap()).unwrap();
    let pin = pinned(&harness);
    let lexical = run_arm(&harness, Arm::Lexical, pin, ACTOR, 20)
        .await
        .unwrap();
    let graph = run_arm(&harness, Arm::LexicalGraph, pin, ACTOR, 20)
        .await
        .unwrap();
    let l = lexical.iter().find(|q| q.query_id == "q0").unwrap();
    let lg = graph.iter().find(|q| q.query_id == "q0").unwrap();
    assert!(!l.parent_ranks.contains(&2) && !l.parent_ranks.contains(&3));
    assert_eq!(&lg.parent_ranks[..4], &[0, 1, 2, 3]);
    assert_eq!(lg.stages.len(), 2);
    assert!(lg.stages[0].retriever_id.ends_with("Lexical"));
    assert!(lg.stages[1].retriever_id.ends_with("HyperGraph"));
    assert!(
        lg.stages[1].raw_parent_ranks.contains(&0),
        "overlap must be deduplicated after S1 federation"
    );
    assert_eq!(lg.parent_ranks.iter().filter(|id| **id == 0).count(), 1);
    for forbidden in [6, 7, 8, 9] {
        assert!(!lg.parent_ranks.contains(&forbidden));
    }
    assert!(
        !lg.stages
            .iter()
            .flat_map(|stage| &stage.raw_parent_ranks)
            .any(|id| [6, 8].contains(id))
    );
    let part_query = graph.iter().find(|q| q.query_id == "qpart").unwrap();
    assert_eq!(
        part_query.parent_ranks,
        vec![0],
        "the second Part's unique body term must retrieve the parent once"
    );
    assert_eq!(part_query.stages[0].raw_parent_ranks, vec![0]);
    let restricted = graph.iter().find(|q| q.query_id == "qaccess").unwrap();
    assert!(restricted.parent_ranks.is_empty());
    assert!(
        restricted
            .stages
            .iter()
            .all(|stage| stage.raw_parent_ranks.is_empty())
    );
    let unrelated = graph.iter().find(|q| q.query_id == "qfalse").unwrap();
    assert_eq!(unrelated.parent_ranks, vec![31]);
}

#[tokio::test]
async fn current_read_and_live_version_revocation_fail_closed() {
    let harness = Harness::build(Corpus::synthetic(32, SEED).unwrap()).unwrap();
    let pin = pinned(&harness);
    let target2 = ResourceId::from_uuid(harness.corpus.records[2].unit.resource_id);
    let target3 = ResourceId::from_uuid(harness.corpus.records[3].unit.resource_id);
    harness.access.set_read(target2, AccessDecision::Unknown);
    harness.access.set_current_version(target3, false);
    let run = run_arm(&harness, Arm::LexicalGraph, pin, ACTOR, 20)
        .await
        .unwrap();
    let q0 = run.iter().find(|q| q.query_id == "q0").unwrap();
    assert!(!q0.parent_ranks.contains(&2) && !q0.parent_ranks.contains(&3));
    assert!(
        !q0.stages
            .iter()
            .flat_map(|stage| &stage.raw_parent_ranks)
            .any(|id| [2, 3].contains(id))
    );
}

#[tokio::test]
async fn mismatched_generation_pin_and_actor_never_run() {
    let harness = Harness::build(Corpus::synthetic(32, SEED).unwrap()).unwrap();
    let mut wrong = pinned(&harness);
    wrong.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(999));
    assert!(
        run_arm(&harness, Arm::LexicalGraph, wrong, ACTOR, 20)
            .await
            .is_err()
    );
    assert!(
        run_arm(
            &harness,
            Arm::LexicalGraph,
            pinned(&harness),
            "other-actor",
            20
        )
        .await
        .is_err()
    );
}

#[test]
fn mock_vector_port_rejects_dimension_nan_stale_binding_and_retention() {
    let corpus = Corpus::synthetic(32, SEED).unwrap();
    let unit = &corpus.records[0].unit;
    let valid_shape = MockVectorHit {
        generation_id: corpus.generation_id,
        parent_resource_id: unit.resource_id,
        source_native_version: unit.source_native_version.clone(),
        source_native_part_id: unit.source_native_part_id.clone(),
        unit_id: unit.unit_id.clone(),
        profile: unit.profile.clone(),
        raw_sha256: unit.raw_sha256.clone(),
        text_sha256: unit.text_sha256.clone(),
        embedding: vec![0.1, 0.2, 0.3],
        authority_scope_matches: true,
        lease_valid: true,
    };
    let mut hit = valid_shape.clone();
    hit.embedding.pop();
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    let mut hit = valid_shape.clone();
    hit.embedding[1] = f32::NAN;
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    let mut hit = valid_shape.clone();
    hit.generation_id = Uuid::from_u128(9);
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    let mut hit = valid_shape.clone();
    hit.text_sha256 = "wrong".into();
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    assert!(
        validate_mock_vector_hit(
            unit,
            &valid_shape,
            corpus.generation_id,
            3,
            RetentionMode::NoRetention,
            true,
            true
        )
        .is_err()
    );
    assert!(
        validate_mock_vector_hit(
            unit,
            &valid_shape,
            corpus.generation_id,
            3,
            RetentionMode::SessionOnly,
            true,
            true
        )
        .is_err()
    );
    let mut hit = valid_shape.clone();
    hit.lease_valid = false;
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    let mut hit = valid_shape;
    hit.authority_scope_matches = false;
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            true
        )
        .is_err()
    );
    hit.authority_scope_matches = true;
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            false,
            true
        )
        .is_err()
    );
    assert!(
        validate_mock_vector_hit(
            unit,
            &hit,
            corpus.generation_id,
            3,
            RetentionMode::PersistentResource,
            true,
            false
        )
        .is_err()
    );
}
