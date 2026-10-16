//! The Candle port of the EmbeddingGemma 2 text tower against the reference
//! implementation: `fixtures/eg2_reference.json` holds the token IDs and
//! normalized embeddings that sentence-transformers (Transformers 5.19,
//! float32 CPU) produced for the pinned files. Runs only where the operator
//! points `SEARCH_EG2_DIR` at those files (`--ignored`).

use search_vector_adapter::{CandleEmbeddingProvider, eg2_spec};

#[derive(serde::Deserialize)]
struct Reference {
    texts: Vec<String>,
    input_ids: Vec<Vec<u32>>,
    embeddings: Vec<Vec<f32>>,
}

fn reference() -> Reference {
    serde_json::from_str(include_str!("fixtures/eg2_reference.json")).unwrap()
}

#[test]
fn pinned_spec_validates_and_differs_from_e5() {
    let id = eg2_spec().validate_and_id().unwrap();
    assert_eq!(id, eg2_spec().validate_and_id().unwrap());
    assert_ne!(
        id,
        search_vector_adapter::e5_small_spec()
            .validate_and_id()
            .unwrap()
    );
}

#[test]
fn reference_texts_carry_the_registered_templates() {
    let spec = eg2_spec();
    let query = spec.query_template.replace("{text}", "");
    let passage = spec.passage_template.replace("{text}", "");
    for text in reference().texts {
        assert!(
            text.starts_with(&query) || text.starts_with(&passage),
            "{text}"
        );
    }
}

#[tokio::test]
#[ignore = "needs the pinned model files in SEARCH_EG2_DIR"]
async fn port_matches_the_reference_embeddings() {
    let dir = std::env::var("SEARCH_EG2_DIR").expect("SEARCH_EG2_DIR");
    let tokenizer = tokenizers::Tokenizer::from_file(format!("{dir}/tokenizer.json")).unwrap();
    let reference = reference();
    for (text, ids) in reference.texts.iter().zip(&reference.input_ids) {
        let encoding = tokenizer.encode(text.as_str(), true).unwrap();
        assert_eq!(encoding.get_ids(), ids.as_slice(), "token IDs of {text}");
    }
    let provider = CandleEmbeddingProvider::load_eg2(&dir).unwrap();
    // One batch of mixed lengths (padding) and each text alone must agree.
    let batched = provider.embed_texts(reference.texts.clone()).await.unwrap();
    for (index, (actual, expected)) in batched.iter().zip(&reference.embeddings).enumerate() {
        assert_eq!(actual.len(), 768);
        let cosine: f32 = actual.iter().zip(expected).map(|(a, b)| a * b).sum();
        let worst = actual
            .iter()
            .zip(expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        eprintln!("eg2 parity text {index}: cosine {cosine:.7}, component error {worst:.2e}");
        assert!(cosine > 0.99999, "text {index}: cosine {cosine}");
        assert!(worst < 1e-4, "text {index}: component error {worst}");
        let alone = provider
            .embed_texts(vec![reference.texts[index].clone()])
            .await
            .unwrap();
        let same: f32 = alone[0].iter().zip(actual).map(|(a, b)| a * b).sum();
        assert!(same > 0.99999, "text {index}: batched vs alone {same}");
    }
}

/// The same mixed-length passages as the E5 order test, for comparing CPU
/// throughput; each result also matches the text embedded alone.
#[tokio::test]
#[ignore = "needs the pinned model files in SEARCH_EG2_DIR"]
async fn port_keeps_input_order_and_reports_throughput() {
    let dir = std::env::var("SEARCH_EG2_DIR").expect("SEARCH_EG2_DIR");
    let provider = CandleEmbeddingProvider::load_eg2(dir).unwrap();
    let texts: Vec<String> = (0..200)
        .map(|n| {
            format!(
                "title: none | text: {}第{n}条の規定。",
                "金融商品取引業者は顧客に対し誠実に業務を行う。".repeat(1 + (n * 7) % 23)
            )
        })
        .collect();
    let started = std::time::Instant::now();
    let together = provider.embed_texts(texts.clone()).await.unwrap();
    let elapsed = started.elapsed();
    eprintln!(
        "embedded {} mixed-length passages in {:.1} s ({:.1}/s)",
        texts.len(),
        elapsed.as_secs_f64(),
        texts.len() as f64 / elapsed.as_secs_f64()
    );
    let cosine = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    for index in [0usize, 13, 101, 199] {
        let alone = provider
            .embed_texts(vec![texts[index].clone()])
            .await
            .unwrap();
        assert!(cosine(&together[index], &alone[0]) > 0.9999, "text {index}");
    }
}
