#[path = "../fixtures/graph/generate.rs"]
mod generate;

use generate::{DegreeProfile, synthetic_graph};
use search_discovery_poc::graph_backend::{PostgresIncidence, QueryExpansion};
use search_discovery_poc::hypergraph::{IncidenceIndex, Relation, TraversalQuery, TraversalStep};
use search_discovery_poc::report::POSTGRES_IMAGE_TAG;
use std::time::Instant;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

fn correctness_relations() -> Vec<Relation> {
    serde_json::from_str(include_str!("../fixtures/graph/relations.json")).unwrap()
}

fn query(
    seed: &str,
    namespace: &str,
    relation_type: &str,
    from_role: &str,
    to_role: &str,
) -> TraversalQuery {
    TraversalQuery {
        seed_resource_id: seed.into(),
        steps: vec![TraversalStep {
            namespace: namespace.into(),
            relation_type: relation_type.into(),
            from_role: from_role.into(),
            to_role: to_role.into(),
            required_participants: Vec::new(),
        }],
        max_paths: 4096,
        max_branching_per_node: 4096,
    }
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn measure_reference(index: &IncidenceIndex, case: &TraversalQuery) -> f64 {
    median(
        (0..25)
            .map(|_| {
                let started = Instant::now();
                index.traverse(case).unwrap();
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect(),
    )
}

async fn measure_postgres(
    index: &PostgresIncidence,
    client: &tokio_postgres::Client,
    case: &TraversalQuery,
) -> (f64, QueryExpansion) {
    let mut samples = Vec::new();
    let mut expansion = None;
    for _ in 0..25 {
        let started = Instant::now();
        let (_, observed) = index.traverse_measured(client, case).await.unwrap();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        expansion = Some(observed);
    }
    (median(samples), expansion.unwrap())
}

#[test]
fn generator_has_reproducible_degree_profiles() {
    for profile in [
        DegreeProfile::Sparse,
        DegreeProfile::Moderate,
        DegreeProfile::High,
    ] {
        let first = synthetic_graph(profile, 128);
        let second = synthetic_graph(profile, 128);
        assert!(first.iter().all(|relation| {
            relation.participants.len() >= 3
                && relation
                    .participants
                    .iter()
                    .any(|participant| participant.role == "borrower")
                && relation
                    .participants
                    .iter()
                    .any(|participant| participant.role == "product")
                && relation
                    .participants
                    .iter()
                    .any(|participant| participant.role == "collateral")
        }));
        assert_eq!(
            first
                .iter()
                .map(|relation| &relation.id)
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|relation| &relation.id)
                .collect::<Vec<_>>()
        );
        let index = IncidenceIndex::build(first).unwrap();
        let root_degree = index.by_resource("company-root").len();
        assert_eq!(
            root_degree,
            if matches!(profile, DegreeProfile::High) {
                128
            } else {
                0
            }
        );
    }
}

#[tokio::test]
async fn postgres_incidence_matches_reference_paths_and_reports_feasibility() {
    let container = GenericImage::new("postgres", POSTGRES_IMAGE_TAG)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "search_poc")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let (mut client, connection) = tokio_postgres::connect(
        &format!("host=127.0.0.1 port={port} user=postgres password=postgres dbname=search_poc"),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    tokio::spawn(async move { connection.await.unwrap() });
    let fixture = correctness_relations();
    let reference = IncidenceIndex::build(fixture.clone()).unwrap();
    let postgres = PostgresIncidence::load(&mut client, &fixture)
        .await
        .unwrap();
    let mut cases = vec![
        query("company-a", "discovery", "loan", "borrower", "product"),
        query("product-b", "evidence", "supported_by", "claim", "evidence"),
        query("concept-root", "semantic", "is_a", "parent", "child"),
    ];
    cases[0].steps[0].required_participants =
        vec![search_discovery_poc::hypergraph::Participant::new(
            "collateral",
            "property-y",
        )];
    cases[2].steps[0].required_participants =
        vec![search_discovery_poc::hypergraph::Participant::new(
            "child",
            "concept-3",
        )];
    let mut multi = query("product-b", "discovery", "loan", "product", "borrower");
    multi.steps.push(
        query("company-a", "discovery", "loan", "borrower", "collateral")
            .steps
            .remove(0),
    );
    cases.push(multi);
    let mut false_composite = query("company-a", "discovery", "loan", "borrower", "product");
    false_composite.steps[0].required_participants = vec![
        search_discovery_poc::hypergraph::Participant::new("product", "product-b"),
        search_discovery_poc::hypergraph::Participant::new("collateral", "property-y"),
    ];
    cases.push(false_composite);
    cases.push(query(
        "company-a",
        "discovery",
        "loan",
        "borrower",
        "collateral",
    ));
    for case in &cases {
        let expected = reference.traverse(case).unwrap();
        let actual = postgres.traverse(&client, case).await.unwrap();
        assert_eq!(actual, expected);
    }
    for (name, case) in [
        ("one_relation", &cases[0]),
        ("constrained_multi_step", &cases[3]),
    ] {
        let (pg_p50_ms, expansion) = measure_postgres(&postgres, &client, case).await;
        println!(
            "MEASUREMENT {}",
            serde_json::json!({
                "case": name,
                "relations": fixture.len(),
                "reference_p50_ms": measure_reference(&reference, case),
                "postgres_p50_ms": pg_p50_ms,
                "returned_rows": expansion.returned_rows,
                "expanded_relations": expansion.expanded_relations,
                "expanded_nodes": expansion.expanded_nodes,
            })
        );
    }
    for (profile, count) in [
        (DegreeProfile::Sparse, 128),
        (DegreeProfile::Moderate, 512),
        (DegreeProfile::High, 1024),
    ] {
        let generated = synthetic_graph(profile, count);
        let started = Instant::now();
        let reference = IncidenceIndex::build(generated.clone()).unwrap();
        let reference_build_ms = started.elapsed().as_secs_f64() * 1000.0;
        let postgres = PostgresIncidence::load(&mut client, &generated)
            .await
            .unwrap();
        let seed = match profile {
            DegreeProfile::Sparse => "company-31",
            DegreeProfile::Moderate => "company-3",
            DegreeProfile::High => "company-root",
        };
        let mut case = query(seed, "discovery", "loan", "borrower", "product");
        if matches!(profile, DegreeProfile::High) {
            let mut broad = case.clone();
            broad.max_branching_per_node = 16;
            assert!(reference.traverse(&broad).is_err());
            assert!(postgres.traverse(&client, &broad).await.is_err());
            let mut path_limited = case.clone();
            path_limited.max_paths = 4;
            assert!(reference.traverse(&path_limited).is_err());
            assert!(postgres.traverse(&client, &path_limited).await.is_err());
            case.steps[0].required_participants =
                vec![search_discovery_poc::hypergraph::Participant::new(
                    "product",
                    "product-31",
                )];
        }
        let expected = reference.traverse(&case).unwrap();
        let actual = postgres.traverse(&client, &case).await.unwrap();
        assert_eq!(actual, expected);
        let (pg_p50_ms, expansion) = measure_postgres(&postgres, &client, &case).await;
        println!(
            "MEASUREMENT {}",
            serde_json::json!({
                "profile": format!("{profile:?}"),
                "relations": count,
                "participant_rows": postgres.participant_rows,
                "reference_build_ms": reference_build_ms,
                "reference_estimated_bytes_lower_bound": reference.estimated_bytes(),
                "postgres_load_ms": postgres.load_ms,
                "postgres_table_bytes": postgres.table_bytes,
                "reference_p50_ms": measure_reference(&reference, &case),
                "postgres_p50_ms": pg_p50_ms,
                "returned_rows": expansion.returned_rows,
                "expanded_relations": expansion.expanded_relations,
                "expanded_nodes": expansion.expanded_nodes,
            })
        );
    }
    let mut invalid = cases[0].clone();
    invalid.steps[0].namespace.clear();
    assert!(reference.traverse(&invalid).is_err());
    assert!(postgres.traverse(&client, &invalid).await.is_err());

    let mut duplicate = fixture[0].clone();
    duplicate
        .participants
        .push(duplicate.participants[0].clone());
    assert!(IncidenceIndex::build(vec![duplicate.clone()]).is_err());
    assert!(
        PostgresIncidence::load(&mut client, &[duplicate])
            .await
            .is_err()
    );

    let mut empty_id = fixture[0].clone();
    empty_id.id.clear();
    assert!(IncidenceIndex::build(vec![empty_id.clone()]).is_err());
    assert!(
        PostgresIncidence::load(&mut client, &[empty_id])
            .await
            .is_err()
    );
}
