//! Validation-only import of vector values computed by the same pinned model
//! elsewhere (a GPU machine), so a 10,000-document Vector build does not
//! spend hours embedding on the validation CPU.
//!
//! `vector_seed export OUT.jsonl` writes one `{"d": cache digest, "t": Unit
//! text}` line per Unit of the current generation that a build would embed.
//! `vector_seed import IN.jsonl IN.f32` stores the vectors of those lines
//! (little-endian f32, one row per line, in order) under the same cache
//! digests. The maintainer re-binds and validates every value before use.
//!
//! Environment: `DATABASE_URL` (secret, never logged), `SEARCH_VALIDATION_SOURCE`
//! (the worker's Source file).

use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::search_core::id::SourceId;
use search_application::search_core::source::{
    DiscoverableSource, EnumerationSemantics, RetentionMode,
};
use search_application::search_core::vector::{
    BoundEmbedding, EmbeddingModelSpec, QueryEmbedding, VectorManifestUnit,
};
use search_application::vector::{EmbeddingProvider, TrustedVectorQuery};
use search_runtime::vector_runtime::{
    RegisteredVectorActivation, VectorMaintainer, VectorServices, document_scope_key,
};
use search_runtime::vector_store::{PgVectorGenerations, PgVectorIndex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[derive(Deserialize)]
struct SourceFile {
    source_id: Uuid,
    source_name: String,
    enumeration_semantics: EnumerationSemantics,
}

#[derive(Serialize, Deserialize)]
struct Line {
    d: String,
    t: String,
}

/// The pinned model's contract without its weights: this tool embeds nothing.
struct SpecOnly(EmbeddingModelSpec);

impl EmbeddingProvider for SpecOnly {
    fn spec(&self) -> &EmbeddingModelSpec {
        &self.0
    }

    fn embed_units<'a>(
        &'a self,
        _: &'a [VectorManifestUnit],
    ) -> BoxFuture<'a, Vec<BoundEmbedding>> {
        Box::pin(async {
            Err(SearchError::SourceUnavailable(
                "vector_seed embeds nothing".into(),
            ))
        })
    }

    fn embed_query<'a>(&'a self, _: &'a TrustedVectorQuery) -> BoxFuture<'a, QueryEmbedding> {
        Box::pin(async {
            Err(SearchError::SourceUnavailable(
                "vector_seed embeds nothing".into(),
            ))
        })
    }
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set"))
}

fn sha256_text(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// `reference MODEL_DIR IN.jsonl N OUT.f32`: the first N lines embedded by
/// the worker's own CPU adapter, to compare with the imported vectors.
async fn reference(args: &[String]) -> Result<(), String> {
    let [model_dir, lines, rows, out] = args else {
        return Err("usage: vector_seed reference MODEL_DIR IN.jsonl N OUT.f32".into());
    };
    let rows: usize = rows.parse().map_err(|_| "N is not a number".to_string())?;
    let texts: Vec<String> =
        BufReader::new(std::fs::File::open(lines).map_err(|error| error.to_string())?)
            .lines()
            .take(rows)
            .map(|line| {
                let line = line.map_err(|error| error.to_string())?;
                let line: Line = serde_json::from_str(&line).map_err(|error| error.to_string())?;
                Ok(format!("passage: {}", line.t))
            })
            .collect::<Result<_, String>>()?;
    let provider = search_vector_adapter::CandleEmbeddingProvider::load(model_dir)
        .map_err(|error| format!("model: {error:?}"))?;
    let vectors = provider
        .embed_texts(texts)
        .await
        .map_err(|error| error.to_string())?;
    let bytes: Vec<u8> = vectors
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    std::fs::write(out, bytes).map_err(|error| error.to_string())?;
    println!("embedded {} reference Units", vectors.len());
    Ok(())
}

async fn run(args: Vec<String>) -> Result<(), String> {
    if args.first().map(String::as_str) == Some("reference") {
        return reference(&args[1..]).await;
    }
    let source_file: SourceFile = serde_json::from_slice(
        &std::fs::read(env("SEARCH_VALIDATION_SOURCE")?)
            .map_err(|_| "Source file cannot be read".to_string())?,
    )
    .map_err(|error| format!("Source file: {error}"))?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&env("DATABASE_URL")?)
        .await
        .map_err(|_| "database unavailable".to_string())?;
    let source_id = SourceId::from_uuid(source_file.source_id);
    let provider: Arc<dyn EmbeddingProvider> =
        Arc::new(SpecOnly(search_vector_adapter::e5_small_spec()));
    let model = provider
        .spec()
        .validate_and_id()
        .map_err(|error| format!("model: {error:?}"))?;
    let dimension = provider.spec().dimension;
    match args.as_slice() {
        [command, out] if command == "export" => {
            let services = VectorServices {
                provider: provider.clone(),
                index: Arc::new(PgVectorIndex::new(pool.clone(), 0.0)),
                generations: Arc::new(PgVectorGenerations::new(pool.clone())),
                activations: Arc::new(RegisteredVectorActivation::new(
                    [source_id],
                    provider.as_ref(),
                )),
            };
            let source = DiscoverableSource::new(
                source_id,
                source_file.source_name,
                source_file.enumeration_semantics,
                RetentionMode::PersistentResource,
            );
            let pending = VectorMaintainer::new(pool, source, services)
                .unstored_units()
                .await
                .map_err(|error| format!("unstored Units: {error}"))?;
            let mut writer = BufWriter::new(
                std::fs::File::create(PathBuf::from(out)).map_err(|error| error.to_string())?,
            );
            for (d, t) in &pending {
                serde_json::to_writer(
                    &mut writer,
                    &Line {
                        d: d.clone(),
                        t: t.clone(),
                    },
                )
                .map_err(|error| error.to_string())?;
                writer.write_all(b"\n").map_err(|error| error.to_string())?;
            }
            writer.flush().map_err(|error| error.to_string())?;
            println!("exported {} Units", pending.len());
        }
        [command, lines, vectors] if command == "import" => {
            let digests: Vec<String> =
                BufReader::new(std::fs::File::open(lines).map_err(|error| error.to_string())?)
                    .lines()
                    .map(|line| {
                        let line = line.map_err(|error| error.to_string())?;
                        Ok(serde_json::from_str::<Line>(&line)
                            .map_err(|error| error.to_string())?
                            .d)
                    })
                    .collect::<Result<_, String>>()?;
            let mut bytes = Vec::new();
            std::fs::File::open(vectors)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|error| error.to_string())?;
            let row = dimension * 4;
            if bytes.len() != digests.len() * row {
                return Err(format!(
                    "{} vector bytes for {} lines of dimension {dimension}",
                    bytes.len(),
                    digests.len()
                ));
            }
            let scope = document_scope_key(source_id);
            let mut stored = 0u64;
            for (batch_digests, batch_bytes) in digests.chunks(4_096).zip(bytes.chunks(4_096 * row))
            {
                let rows: Vec<Vec<u8>> = batch_bytes.chunks(row).map(<[u8]>::to_vec).collect();
                for vector in &rows {
                    let norm: f32 = vector
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|chunk| f32::from_le_bytes(*chunk).powi(2))
                        .sum();
                    if !norm.is_finite() || (norm.sqrt() - 1.0).abs() > 1e-3 {
                        return Err("a vector is not L2-normalized".into());
                    }
                }
                let sums: Vec<String> = rows.iter().map(|vector| sha256_text(vector)).collect();
                stored += sqlx::query(
                    "INSERT INTO search_vector_value (model_id,cache_digest,authority_scope_key, \
                     vector,vector_sha256) SELECT $1, d, $2, v, s \
                     FROM UNNEST($3::text[], $4::bytea[], $5::text[]) AS t(d, v, s) \
                     ON CONFLICT (model_id, cache_digest) DO NOTHING",
                )
                .bind(model.as_str())
                .bind(&scope)
                .bind(batch_digests)
                .bind(&rows)
                .bind(&sums)
                .execute(&pool)
                .await
                .map_err(|error| format!("store: {error}"))?
                .rows_affected();
            }
            println!("stored {stored} of {} values", digests.len());
        }
        _ => return Err("usage: vector_seed export OUT.jsonl | import IN.jsonl IN.f32".into()),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("vector_seed: {error}");
            ExitCode::FAILURE
        }
    }
}
