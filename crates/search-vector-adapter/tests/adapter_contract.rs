//! E: the pinned E5 adapter refuses anything but the pinned files, and its
//! registered identity is stable. The real-model check runs only where the
//! operator points `SEARCH_E5_DIR` at the pinned files (`--ignored`).

use search_vector_adapter::{AdapterError, CandleEmbeddingProvider, e5_small_spec};

#[test]
fn pinned_spec_validates_and_its_identity_is_stable() {
    let first = e5_small_spec().validate_and_id().unwrap();
    let second = e5_small_spec().validate_and_id().unwrap();
    assert_eq!(first, second);
    let mut other = e5_small_spec();
    other.passage_template = "{text}".into();
    assert_ne!(other.validate_and_id().unwrap(), first);
}

#[test]
fn missing_or_substituted_files_never_load() {
    let dir = std::env::temp_dir().join(format!("e5-missing-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(
        CandleEmbeddingProvider::load(&dir),
        Err(AdapterError::Missing("model.safetensors"))
    ));
    for name in ["model.safetensors", "tokenizer.json", "config.json"] {
        std::fs::write(dir.join(name), b"not the pinned bytes").unwrap();
    }
    assert!(matches!(
        CandleEmbeddingProvider::load(&dir),
        Err(AdapterError::Digest("model.safetensors"))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
#[ignore = "needs the pinned model files in SEARCH_E5_DIR"]
async fn real_model_embeds_unit_vectors_and_ranks_a_paraphrase_first() {
    let dir = std::env::var("SEARCH_E5_DIR").expect("SEARCH_E5_DIR");
    let provider = CandleEmbeddingProvider::load(dir).unwrap();
    let vectors = provider
        .embed_texts(vec![
            "query: 社員の休暇の申請方法".into(),
            "passage: 年次有給休暇を取得するときは、所属長に事前に届け出る。".into(),
            "passage: 会議室の予約は総務の予約システムで行う。".into(),
        ])
        .await
        .unwrap();
    for vector in &vectors {
        assert_eq!(vector.len(), 384);
        let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3);
    }
    let cosine = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    assert!(cosine(&vectors[0], &vectors[1]) > cosine(&vectors[0], &vectors[2]));
}
