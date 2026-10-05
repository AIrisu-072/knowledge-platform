//! The two pinned 384-dimensional models on Candle CPU (P2-02).
//!
//! Each model keeps its own contract: E5 uses `query: ` / `passage: ` and a
//! 512-token limit, MiniLM no prefix and 128 tokens. Both use attention-mask
//! mean pooling over the last hidden state. The raw pooled vector is kept
//! for parity; retrieval uses a separate L2-normalized copy.

use std::path::Path;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelId {
    E5,
    MiniLm,
}

impl ModelId {
    pub const ALL: [Self; 2] = [Self::E5, Self::MiniLm];

    pub fn name(self) -> &'static str {
        match self {
            Self::E5 => "e5",
            Self::MiniLm => "minilm",
        }
    }

    pub fn max_tokens(self) -> usize {
        match self {
            Self::E5 => 512,
            Self::MiniLm => 128,
        }
    }

    pub fn prefix(self, kind: TextKind) -> &'static str {
        match (self, kind) {
            (Self::E5, TextKind::Query) => "query: ",
            (Self::E5, TextKind::Passage) => "passage: ",
            (Self::MiniLm, _) => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Query,
    Passage,
}

#[derive(Debug, Clone)]
pub struct Embedded {
    /// Unpadded token IDs including special tokens.
    pub token_ids: Vec<Vec<u32>>,
    /// Attention-masked mean of the last hidden state.
    pub raw: Vec<Vec<f32>>,
    /// L2-normalized copy for cosine retrieval.
    pub normalized: Vec<Vec<f32>>,
}

pub struct Embedder {
    model: ModelId,
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

pub fn l2(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    values.iter().map(|value| value / norm).collect()
}

impl Embedder {
    /// `root` is the PoC directory holding `metadata/` and `assets/`.
    pub fn load(model: ModelId, root: &Path) -> Result<Self, String> {
        let device = Device::Cpu;
        let config: Config = serde_json::from_slice(
            &std::fs::read(root.join("metadata").join(model.name()).join("config.json"))
                .map_err(error)?,
        )
        .map_err(error)?;
        let assets = root.join("assets").join(model.name());
        // SAFETY: the weights file is a hash-checked, read-only local asset.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[assets.join("model.safetensors")],
                DType::F32,
                &device,
            )
        }
        .map_err(error)?;
        let bert = BertModel::load(vb, &config).map_err(error)?;
        let mut tokenizer = Tokenizer::from_file(assets.join("tokenizer.json")).map_err(error)?;
        let pad_id = tokenizer
            .token_to_id("<pad>")
            .ok_or("tokenizer has no <pad> token")?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id,
            pad_token: "<pad>".into(),
            ..PaddingParams::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: model.max_tokens(),
                ..TruncationParams::default()
            }))
            .map_err(error)?;
        Ok(Self {
            model,
            bert,
            tokenizer,
            device,
        })
    }

    pub fn model(&self) -> ModelId {
        self.model
    }

    pub fn embed(&self, texts: &[&str], kind: TextKind) -> Result<Embedded, String> {
        if texts.is_empty() {
            return Ok(Embedded {
                token_ids: vec![],
                raw: vec![],
                normalized: vec![],
            });
        }
        let prefixed: Vec<String> = texts
            .iter()
            .map(|text| format!("{}{text}", self.model.prefix(kind)))
            .collect();
        let encodings = self.tokenizer.encode_batch(prefixed, true).map_err(error)?;
        let batch = encodings.len();
        let seq = encodings[0].get_ids().len();
        let mut ids = Vec::with_capacity(batch * seq);
        let mut mask = Vec::with_capacity(batch * seq);
        let mut token_ids = Vec::with_capacity(batch);
        for encoding in &encodings {
            ids.extend_from_slice(encoding.get_ids());
            mask.extend_from_slice(encoding.get_attention_mask());
            token_ids.push(
                encoding
                    .get_ids()
                    .iter()
                    .zip(encoding.get_attention_mask())
                    .filter(|(_, mask)| **mask == 1)
                    .map(|(id, _)| *id)
                    .collect(),
            );
        }
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
        let raw: Vec<Vec<f32>> = summed
            .broadcast_div(&counts)
            .and_then(|mean| mean.to_vec2())
            .map_err(error)?;
        let normalized = raw.iter().map(|values| l2(values)).collect();
        Ok(Embedded {
            token_ids,
            raw,
            normalized,
        })
    }
}
