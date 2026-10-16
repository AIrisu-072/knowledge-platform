//! The text tower of EmbeddingGemma 2 (`embedding_gemma2`) on Candle.
//!
//! A port of the Transformers encoder: bidirectional attention on every
//! layer, per-layer embeddings projected from the token embeddings, query,
//! key and value normalization, two RoPE bases and a final projection to the
//! embedding dimension. Only `language_model.*` weights are read. Inputs are
//! at most `MAX_TOKENS` long, which the sliding window of 512 covers whole,
//! so sliding and full layers differ only in their head shape and RoPE base.

use std::collections::HashMap;

use candle_core::{D, DType, Device, Module, Result, Tensor};
use candle_nn::{Embedding, Linear, VarBuilder, embedding, linear_no_bias};
use serde::Deserialize;

/// The longest input; the sliding window covers it, so no window mask is
/// needed.
pub const MAX_TOKENS: usize = 512;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub text_config: TextConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextConfig {
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub hidden_size_per_layer_input: usize,
    pub embedding_dim: usize,
    pub rms_norm_eps: f64,
    pub vocab_size: usize,
    pub sliding_window: usize,
    pub hidden_activation: String,
    pub attention_bias: bool,
    pub layer_types: Vec<String>,
    #[serde(default)]
    pub per_layer_config: HashMap<String, LayerOverride>,
    pub rope_parameters: HashMap<String, RopeParameters>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayerOverride {
    pub head_dim: usize,
    pub num_key_value_heads: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RopeParameters {
    pub rope_theta: f64,
    pub rope_type: String,
}

impl TextConfig {
    /// Checks the parts of the configuration this port implements.
    pub fn check(&self) -> std::result::Result<(), String> {
        if self.hidden_activation != "gelu_pytorch_tanh" {
            return Err(format!("activation {}", self.hidden_activation));
        }
        if self.attention_bias {
            return Err("attention bias".into());
        }
        if self.sliding_window < MAX_TOKENS {
            return Err(format!("sliding window {}", self.sliding_window));
        }
        if self.layer_types.len() != self.num_hidden_layers {
            return Err("layer types".into());
        }
        for kind in &self.layer_types {
            match self.rope_parameters.get(kind) {
                Some(rope) if rope.rope_type == "default" => {}
                _ => return Err(format!("RoPE of {kind}")),
            }
        }
        Ok(())
    }

    /// Head size and key/value heads of layer `index`.
    fn layer_shape(&self, index: usize) -> (usize, usize) {
        match self.per_layer_config.get(&format!("{index:02}")) {
            Some(layer) => (layer.head_dim, layer.num_key_value_heads),
            None => (self.head_dim, self.num_key_value_heads),
        }
    }
}

/// `x * (mean(x^2) + eps)^-0.5`, then the weight when there is one.
struct RmsNorm {
    weight: Option<Tensor>,
    eps: f64,
}

impl RmsNorm {
    fn new(dim: usize, eps: f64, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            weight: Some(vb.get(dim, "weight")?),
            eps,
        })
    }

    fn unscaled(eps: f64) -> Self {
        Self { weight: None, eps }
    }
}

impl Module for RmsNorm {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mean = xs.sqr()?.mean_keepdim(D::Minus1)?;
        let normed = xs.broadcast_div(&(mean + self.eps)?.sqrt()?)?;
        match &self.weight {
            Some(weight) => normed.broadcast_mul(weight),
            None => Ok(normed),
        }
    }
}

struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    v_norm: RmsNorm,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
}

impl Attention {
    fn new(cfg: &TextConfig, index: usize, vb: VarBuilder) -> Result<Self> {
        let (head_dim, kv_heads) = cfg.layer_shape(index);
        let heads = cfg.num_attention_heads;
        let hidden = cfg.hidden_size;
        Ok(Self {
            q_proj: linear_no_bias(hidden, heads * head_dim, vb.pp("q_proj"))?,
            k_proj: linear_no_bias(hidden, kv_heads * head_dim, vb.pp("k_proj"))?,
            v_proj: linear_no_bias(hidden, kv_heads * head_dim, vb.pp("v_proj"))?,
            o_proj: linear_no_bias(heads * head_dim, hidden, vb.pp("o_proj"))?,
            q_norm: RmsNorm::new(head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))?,
            k_norm: RmsNorm::new(head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))?,
            v_norm: RmsNorm::unscaled(cfg.rms_norm_eps),
            heads,
            kv_heads,
            head_dim,
        })
    }

    /// `mask` is `(batch, 1, 1, seq)`: 0 for tokens, -inf for padding.
    fn forward(&self, xs: &Tensor, rope: &Rope, mask: &Tensor) -> Result<Tensor> {
        let (batch, seq, _) = xs.dims3()?;
        let split = |xs: Tensor, heads: usize| {
            xs.reshape((batch, seq, heads, self.head_dim))?
                .transpose(1, 2)?
                .contiguous()
        };
        let q = self
            .q_norm
            .forward(&split(self.q_proj.forward(xs)?, self.heads)?)?;
        let k = self
            .k_norm
            .forward(&split(self.k_proj.forward(xs)?, self.kv_heads)?)?;
        let v = self
            .v_norm
            .forward(&split(self.v_proj.forward(xs)?, self.kv_heads)?)?;
        let q = rope.apply(&q)?;
        let k = rope.apply(&k)?;
        let groups = self.heads / self.kv_heads;
        let k = repeat_kv(k, groups)?;
        let v = repeat_kv(v, groups)?;
        // Scaling 1.0: the query and key norms take the place of 1/sqrt(d).
        let scores = q.matmul(&k.t()?)?.broadcast_add(mask)?;
        let weights = candle_nn::ops::softmax_last_dim(&scores)?;
        let out = weights.matmul(&v)?.transpose(1, 2)?.reshape((
            batch,
            seq,
            self.heads * self.head_dim,
        ))?;
        self.o_proj.forward(&out)
    }
}

fn repeat_kv(xs: Tensor, groups: usize) -> Result<Tensor> {
    if groups == 1 {
        return Ok(xs);
    }
    let (batch, kv_heads, seq, dim) = xs.dims4()?;
    xs.unsqueeze(2)?
        .expand((batch, kv_heads, groups, seq, dim))?
        .reshape((batch, kv_heads * groups, seq, dim))
}

/// Non-interleaved RoPE (`rotate_half`) for one head size and base.
struct Rope {
    cos: Tensor,
    sin: Tensor,
}

impl Rope {
    fn new(head_dim: usize, theta: f64, seq: usize, device: &Device) -> Result<Self> {
        let inv: Vec<f32> = (0..head_dim)
            .step_by(2)
            .map(|i| 1.0 / theta.powf(i as f64 / head_dim as f64) as f32)
            .collect();
        let inv = Tensor::from_vec(inv, (1, head_dim / 2), device)?;
        let positions = Tensor::arange(0u32, seq as u32, device)?
            .to_dtype(DType::F32)?
            .reshape((seq, 1))?;
        let freqs = positions.matmul(&inv)?;
        Ok(Self {
            cos: freqs.cos()?,
            sin: freqs.sin()?,
        })
    }

    fn apply(&self, xs: &Tensor) -> Result<Tensor> {
        candle_nn::rotary_emb::rope(xs, &self.cos, &self.sin)
    }
}

struct Mlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
}

impl Mlp {
    fn new(cfg: &TextConfig, vb: VarBuilder) -> Result<Self> {
        let (hidden, inner) = (cfg.hidden_size, cfg.intermediate_size);
        Ok(Self {
            gate_proj: linear_no_bias(hidden, inner, vb.pp("gate_proj"))?,
            up_proj: linear_no_bias(hidden, inner, vb.pp("up_proj"))?,
            down_proj: linear_no_bias(inner, hidden, vb.pp("down_proj"))?,
        })
    }
}

impl Module for Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let gate = self.gate_proj.forward(xs)?.gelu()?;
        self.down_proj.forward(&(gate * self.up_proj.forward(xs)?)?)
    }
}

/// Mixes one layer's slice of the per-layer embeddings into the residual.
struct PleBlock {
    gate: Linear,
    projection: Linear,
    norm: RmsNorm,
}

impl PleBlock {
    fn new(cfg: &TextConfig, vb: VarBuilder) -> Result<Self> {
        let (hidden, ple) = (cfg.hidden_size, cfg.hidden_size_per_layer_input);
        Ok(Self {
            gate: linear_no_bias(hidden, ple, vb.pp("per_layer_input_gate"))?,
            projection: linear_no_bias(ple, hidden, vb.pp("per_layer_projection"))?,
            norm: RmsNorm::new(hidden, cfg.rms_norm_eps, vb.pp("post_per_layer_input_norm"))?,
        })
    }

    fn forward(&self, xs: &Tensor, per_layer: &Tensor) -> Result<Tensor> {
        let gated = (self.gate.forward(xs)?.gelu()? * per_layer)?;
        xs + self.norm.forward(&self.projection.forward(&gated)?)?
    }
}

struct Layer {
    attention: Attention,
    mlp: Mlp,
    input_norm: RmsNorm,
    post_attention_norm: RmsNorm,
    pre_feedforward_norm: RmsNorm,
    post_feedforward_norm: RmsNorm,
    ple: PleBlock,
    scalar: Tensor,
    full: bool,
}

impl Layer {
    fn new(cfg: &TextConfig, index: usize, vb: VarBuilder) -> Result<Self> {
        let norm = |name: &str| RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp(name));
        Ok(Self {
            attention: Attention::new(cfg, index, vb.pp("self_attn"))?,
            mlp: Mlp::new(cfg, vb.pp("mlp"))?,
            input_norm: norm("input_layernorm")?,
            post_attention_norm: norm("post_attention_layernorm")?,
            pre_feedforward_norm: norm("pre_feedforward_layernorm")?,
            post_feedforward_norm: norm("post_feedforward_layernorm")?,
            ple: PleBlock::new(cfg, vb.pp("ple_block"))?,
            scalar: vb.get(1, "layer_scalar")?,
            full: cfg.layer_types[index] == "full_attention",
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        per_layer: &Tensor,
        rope: &Rope,
        mask: &Tensor,
    ) -> Result<Tensor> {
        let attended = self
            .attention
            .forward(&self.input_norm.forward(xs)?, rope, mask)?;
        let xs = (xs + self.post_attention_norm.forward(&attended)?)?;
        let fed = self.mlp.forward(&self.pre_feedforward_norm.forward(&xs)?)?;
        let xs = (&xs + self.post_feedforward_norm.forward(&fed)?)?;
        self.ple
            .forward(&xs, per_layer)?
            .broadcast_mul(&self.scalar)
    }
}

/// The text encoder: token IDs to per-token embeddings of `embedding_dim`.
pub struct TextModel {
    embed_tokens: Embedding,
    embed_scale: f64,
    ple_projection: Linear,
    ple_scale: f64,
    ple_norm: RmsNorm,
    layers: Vec<Layer>,
    norm: RmsNorm,
    embedding_projection: Linear,
    cfg: TextConfig,
}

impl TextModel {
    /// `vb` points at the `language_model` weights.
    pub fn new(cfg: &TextConfig, vb: VarBuilder) -> Result<Self> {
        let layers = (0..cfg.num_hidden_layers)
            .map(|index| Layer::new(cfg, index, vb.pp(format!("layers.{index}"))))
            .collect::<Result<Vec<_>>>()?;
        let ple = cfg.num_hidden_layers * cfg.hidden_size_per_layer_input;
        Ok(Self {
            embed_tokens: embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("embed_tokens"))?,
            embed_scale: (cfg.hidden_size as f64).sqrt(),
            ple_projection: linear_no_bias(
                cfg.hidden_size,
                ple,
                vb.pp("ple.per_layer_model_projection"),
            )?,
            ple_scale: (cfg.hidden_size as f64).powf(-0.5),
            ple_norm: RmsNorm::new(
                cfg.hidden_size_per_layer_input,
                cfg.rms_norm_eps,
                vb.pp("ple.per_layer_projection_norm"),
            )?,
            layers,
            norm: RmsNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))?,
            embedding_projection: linear_no_bias(
                cfg.hidden_size,
                cfg.embedding_dim,
                vb.pp("embedding_projection"),
            )?,
            cfg: cfg.clone(),
        })
    }

    /// `ids` and `attention_mask` are `(batch, seq)` (`seq` at most
    /// [`MAX_TOKENS`]); the result is `(batch, seq, embedding_dim)`.
    pub fn forward(&self, ids: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        let (batch, seq) = ids.dims2()?;
        if seq > MAX_TOKENS {
            candle_core::bail!("input of {seq} tokens is longer than {MAX_TOKENS}");
        }
        let device = ids.device();
        let xs = (self.embed_tokens.forward(ids)? * self.embed_scale)?;
        let per_layer = (self.ple_projection.forward(&xs)? * self.ple_scale)?.reshape((
            batch,
            seq,
            self.cfg.num_hidden_layers,
            self.cfg.hidden_size_per_layer_input,
        ))?;
        let per_layer = self.ple_norm.forward(&per_layer)?;
        // 0 where a key is a token, -1e30 (zero weight) where it is padding.
        let mask = attention_mask
            .to_dtype(DType::F32)?
            .affine(1e30, -1e30)?
            .reshape((batch, 1, 1, seq))?;
        let rope_for = |kind: &str, head_dim: usize| -> Result<Rope> {
            let theta = self.cfg.rope_parameters[kind].rope_theta;
            Rope::new(head_dim, theta, seq, device)
        };
        let (sliding_dim, _) = self.cfg.layer_shape(
            self.layers
                .iter()
                .position(|layer| !layer.full)
                .unwrap_or_default(),
        );
        let sliding = rope_for("sliding_attention", sliding_dim)?;
        let full = match self.layers.iter().position(|layer| layer.full) {
            Some(index) => Some(rope_for("full_attention", self.cfg.layer_shape(index).0)?),
            None => None,
        };
        let mut xs = xs;
        for (index, layer) in self.layers.iter().enumerate() {
            let rope = match (&full, layer.full) {
                (Some(full), true) => full,
                _ => &sliding,
            };
            let slice = per_layer.narrow(2, index, 1)?.squeeze(2)?;
            xs = layer.forward(&xs, &slice, rope, &mask)?;
        }
        self.embedding_projection.forward(&self.norm.forward(&xs)?)
    }
}
