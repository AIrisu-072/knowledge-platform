//! P2-02 parity: the Candle CPU embeddings of both pinned models against
//! the test-only `transformers` oracle in `fixtures/reference-vectors.json`.
//! Tolerances are the protocol's: exact token IDs, component error <= 0.002,
//! cosine error <= 0.0001, retrieval norm in [0.9999, 1.0001], and every
//! reference order with a cosine margin > 0.0002 kept.

use std::path::PathBuf;

use search_vector_model_poc::embed::{Embedder, ModelId, TextKind};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn reference() -> Value {
    serde_json::from_slice(&std::fs::read(root().join("fixtures/reference-vectors.json")).unwrap())
        .unwrap()
}

fn floats(value: &Value) -> Vec<f32> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_f64().unwrap() as f32)
        .collect()
}

fn ids(value: &Value) -> Vec<u32> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_u64().unwrap() as u32)
        .collect()
}

fn kind(value: &Value) -> TextKind {
    match value.as_str().unwrap() {
        "query" => TextKind::Query,
        _ => TextKind::Passage,
    }
}

fn cosine(left: &[f32], right: &[f32]) -> f64 {
    let dot: f64 = left
        .iter()
        .zip(right)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let norm = |values: &[f32]| {
        values
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    dot / (norm(left) * norm(right))
}

fn assert_close(model: ModelId, case: &Value, token_ids: &[u32], raw: &[f32], normalized: &[f32]) {
    let label = format!(
        "{} {:?}",
        model.name(),
        &case["text"]
            .as_str()
            .unwrap()
            .chars()
            .take(24)
            .collect::<String>()
    );
    assert_eq!(
        token_ids,
        ids(&case["token_ids"]).as_slice(),
        "{label} token IDs"
    );
    assert!(token_ids.len() <= model.max_tokens(), "{label} token limit");
    for (name, ours, theirs) in [
        ("raw", raw, floats(&case["raw"])),
        ("normalized", normalized, floats(&case["normalized"])),
    ] {
        assert_eq!(ours.len(), 384, "{label} {name} dimension");
        assert!(
            ours.iter().all(|value| value.is_finite()),
            "{label} {name} finite"
        );
        let max_error = ours
            .iter()
            .zip(&theirs)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_error <= 0.002,
            "{label} {name} component error {max_error}"
        );
        let cosine_error = (1.0 - cosine(ours, &theirs)).abs();
        assert!(
            cosine_error <= 0.0001,
            "{label} {name} cosine error {cosine_error}"
        );
    }
    let norm = normalized.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((0.9999..=1.0001).contains(&norm), "{label} norm {norm}");
}

fn check(model: ModelId) {
    let fixture = &reference()["models"][model.name()];
    let embedder = Embedder::load(model, &root()).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    let mut ours = Vec::new();
    for case in cases {
        let embedded = embedder
            .embed(&[case["text"].as_str().unwrap()], kind(&case["kind"]))
            .unwrap();
        assert_close(
            model,
            case,
            &embedded.token_ids[0],
            &embedded.raw[0],
            &embedded.normalized[0],
        );
        ours.push(embedded.normalized[0].clone());
    }
    // A padded batch reproduces each unpadded member.
    let batch = fixture["batch"].as_array().unwrap();
    let texts: Vec<&str> = batch
        .iter()
        .map(|case| case["text"].as_str().unwrap())
        .collect();
    let embedded = embedder.embed(&texts, TextKind::Passage).unwrap();
    for (index, case) in batch.iter().enumerate() {
        assert_close(
            model,
            case,
            &embedded.token_ids[index],
            &embedded.raw[index],
            &embedded.normalized[index],
        );
    }
    // Every query keeps each reference order with a margin above 0.0002.
    let references: Vec<Vec<f32>> = cases
        .iter()
        .map(|case| floats(&case["normalized"]))
        .collect();
    for (query, case) in cases.iter().enumerate() {
        if case["kind"] != "query" {
            continue;
        }
        for left in 0..cases.len() {
            for right in 0..cases.len() {
                if cases[left]["kind"] != "passage" || cases[right]["kind"] != "passage" {
                    continue;
                }
                let margin = cosine(&references[query], &references[left])
                    - cosine(&references[query], &references[right]);
                if margin > 0.0002 {
                    assert!(
                        cosine(&ours[query], &ours[left]) > cosine(&ours[query], &ours[right]),
                        "{} order reversed for query {query}: {left} vs {right}",
                        model.name()
                    );
                }
            }
        }
    }
}

#[test]
fn e5_candle_cpu_matches_reference() {
    check(ModelId::E5);
}

#[test]
fn minilm_candle_cpu_matches_reference() {
    check(ModelId::MiniLm);
}
