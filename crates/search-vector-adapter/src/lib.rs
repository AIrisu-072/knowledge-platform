//! E (P2-07): the selected Vector embedding adapter.
//!
//! The pinned `intfloat/multilingual-e5-small` model runs on Candle CPU. The
//! operator supplies a directory holding exactly the pinned files; every file
//! is hash-checked before anything is loaded, and the model identity binds
//! those digests, the templates, pooling, normalization and the runtime
//! build. Inputs use the model's own contract: `query: ` / `passage: `
//! prefixes, a 512-token limit after the prefix, attention-masked mean
//! pooling of the last hidden state and an L2-normalized result for cosine
//! retrieval. Inference runs on a blocking thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::vector::{EmbeddingProvider, TrustedVectorQuery};
use search_core::vector::{
    BoundEmbedding, EmbeddingModelSpec, QueryEmbedding, VectorManifestUnit, VectorMetric,
    VectorNormalization, VectorPrecision,
};
use sha2::{Digest, Sha256};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

/// The pinned model repository revision.
pub const E5_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
/// Units embedded per forward pass.
const BATCH: usize = 16;

fn hex(value: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap_or_default();
    }
    out
}

/// One pinned file of the model directory.
struct Pinned {
    name: &'static str,
    sha256: &'static str,
}

const FILES: [Pinned; 3] = [
    Pinned {
        name: "model.safetensors",
        sha256: "1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477",
    },
    Pinned {
        name: "tokenizer.json",
        sha256: "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    },
    Pinned {
        name: "config.json",
        sha256: "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
    },
];
/// `tokenizer_config.json` of the pinned revision.
const TOKENIZER_CONFIG_SHA256: &str =
    "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b";

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("model file {0} is missing or unreadable")]
    Missing(&'static str),
    #[error("model file {0} does not match its pinned digest")]
    Digest(&'static str),
    #[error("model load failed: {0}")]
    Load(String),
}

/// The registered specification of the pinned E5 model on this runtime.
pub fn e5_small_spec() -> EmbeddingModelSpec {
    EmbeddingModelSpec {
        model_name: "intfloat/multilingual-e5-small".into(),
        model_revision: E5_REVISION.into(),
        weights_sha256: hex(FILES[0].sha256),
        tokenizer_revision: E5_REVISION.into(),
        tokenizer_files_sha256: hex(FILES[1].sha256),
        tokenizer_config_sha256: hex(TOKENIZER_CONFIG_SHA256),
        unicode_preprocessing: "pinned tokenizer.json normalizer".into(),
        input_preprocessing: "prefix then tokenizer with special tokens".into(),
        query_template: "query: {text}".into(),
        passage_template: "passage: {text}".into(),
        pooling: "attention-masked mean of the last hidden state".into(),
        attention_masking: "tokenizer attention mask, batch-longest padding".into(),
        max_tokens: 512,
        chunking: "one P1 Unit per embedding".into(),
        truncation: "tokenizer truncation to 512 tokens after the prefix".into(),
        dimension: 384,
        precision: VectorPrecision::F32,
        normalization: VectorNormalization::UnitL2,
        metric: VectorMetric::Cosine,
        runtime_family: "candle-cpu".into(),
        runtime_build: "candle-0.11.0+tokenizers-0.22.0".into(),
        native_binary_sha256: None,
        deterministic_config_sha256: hex(FILES[2].sha256),
    }
}

fn verify(dir: &Path) -> Result<(), AdapterError> {
    for file in &FILES {
        let bytes =
            std::fs::read(dir.join(file.name)).map_err(|_| AdapterError::Missing(file.name))?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != file.sha256 {
            return Err(AdapterError::Digest(file.name));
        }
    }
    Ok(())
}

struct Model {
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl Model {
    fn load(dir: &Path) -> Result<Self, AdapterError> {
        verify(dir)?;
        let load = |error: &dyn std::fmt::Display| AdapterError::Load(error.to_string());
        let device = Device::Cpu;
        let config: Config = serde_json::from_slice(
            &std::fs::read(dir.join("config.json"))
                .map_err(|_| AdapterError::Missing("config.json"))?,
        )
        .map_err(|error| load(&error))?;
        // SAFETY: the weights file was hash-checked above and is read-only.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                DType::F32,
                &device,
            )
        }
        .map_err(|error| load(&error))?;
        let bert = BertModel::load(vb, &config).map_err(|error| load(&error))?;
        let mut tokenizer =
            Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|error| load(&error))?;
        let pad_id = tokenizer
            .token_to_id("<pad>")
            .ok_or_else(|| AdapterError::Load("tokenizer has no <pad> token".into()))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id,
            pad_token: "<pad>".into(),
            ..PaddingParams::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: 512,
                ..TruncationParams::default()
            }))
            .map_err(|error| load(&error))?;
        Ok(Self {
            bert,
            tokenizer,
            device,
        })
    }

    /// L2-normalized attention-masked mean embeddings, in input order.
    fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH) {
            let encodings = self
                .tokenizer
                .encode_batch(chunk.to_vec(), true)
                .map_err(|error| error.to_string())?;
            let batch = encodings.len();
            let seq = encodings[0].get_ids().len();
            let mut ids = Vec::with_capacity(batch * seq);
            let mut mask = Vec::with_capacity(batch * seq);
            for encoding in &encodings {
                ids.extend_from_slice(encoding.get_ids());
                mask.extend_from_slice(encoding.get_attention_mask());
            }
            let error = |error: candle_core::Error| error.to_string();
            let input = Tensor::from_vec(ids, (batch, seq), &self.device).map_err(error)?;
            let mask = Tensor::from_vec(mask, (batch, seq), &self.device).map_err(error)?;
            let types = input.zeros_like().map_err(error)?;
            let hidden = self
                .bert
                .forward(&input, &types, Some(&mask))
                .map_err(error)?;
            let weights = mask
                .to_dtype(DType::F32)
                .and_then(|mask| mask.unsqueeze(2))
                .map_err(error)?;
            let summed = hidden
                .broadcast_mul(&weights)
                .and_then(|masked| masked.sum(1))
                .map_err(error)?;
            let counts = weights.sum(1).map_err(error)?;
            let pooled: Vec<Vec<f32>> = summed
                .broadcast_div(&counts)
                .and_then(|mean| mean.to_vec2())
                .map_err(error)?;
            for values in pooled {
                let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
                out.push(values.iter().map(|value| value / norm).collect());
            }
        }
        Ok(out)
    }
}

/// The pinned E5 model as the registered [`EmbeddingProvider`].
pub struct CandleEmbeddingProvider {
    spec: EmbeddingModelSpec,
    model: Arc<Model>,
}

impl CandleEmbeddingProvider {
    /// Loads the model from `dir` after checking every pinned file.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self, AdapterError> {
        let dir = dir.into();
        Ok(Self {
            spec: e5_small_spec(),
            model: Arc::new(Model::load(&dir)?),
        })
    }

    /// Embeds texts that already carry their template, in input order.
    pub async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, SearchError> {
        self.run(texts).await
    }

    async fn run(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, SearchError> {
        let model = self.model.clone();
        tokio::task::spawn_blocking(move || model.embed(texts))
            .await
            .map_err(|_| SearchError::OperationFailed("embedding task failed".into()))?
            .map_err(|error| SearchError::OperationFailed(format!("embedding failed: {error}")))
    }
}

impl EmbeddingProvider for CandleEmbeddingProvider {
    fn spec(&self) -> &EmbeddingModelSpec {
        &self.spec
    }

    fn embed_units<'a>(
        &'a self,
        units: &'a [VectorManifestUnit],
    ) -> BoxFuture<'a, Vec<BoundEmbedding>> {
        Box::pin(async move {
            let texts = units
                .iter()
                .map(|item| {
                    self.spec
                        .passage_template
                        .replace("{text}", &item.unit.text)
                })
                .collect();
            let values = self.run(texts).await?;
            units
                .iter()
                .zip(values)
                .map(|(item, values)| {
                    BoundEmbedding::new(&self.spec, &item.unit, &item.authority, values)
                        .map_err(|_| SearchError::OperationFailed("embedding binding".into()))
                })
                .collect()
        })
    }

    fn embed_query<'a>(&'a self, query: &'a TrustedVectorQuery) -> BoxFuture<'a, QueryEmbedding> {
        Box::pin(async move {
            let text = self.spec.query_template.replace("{text}", query.text());
            let mut values = self.run(vec![text]).await?;
            QueryEmbedding::new(&self.spec, values.pop().unwrap_or_default())
                .map_err(|_| SearchError::OperationFailed("query embedding".into()))
        })
    }
}
