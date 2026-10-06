//! E (2026-10-06): the pre-registered MIRACL ja dev public lane.
//!
//! The lane file (built by `scripts/prepare_public_ja.py`, outside the
//! repository) holds 140 judged-positive dev queries — 40 calibration, 100
//! evaluation — and the union of their judged passages. Lexical arms use
//! Tantivy over title and text: `prod` is the production behaviour (default
//! tokenizer, literal phrase), `bm25` the same tokenizer as an OR query, and
//! `bigram` a character-bigram field as a stronger sensitivity baseline.
//! Dense is exact cosine over E5 passage embeddings with the floor `tau`.
//! A no-answer variant ranks each query with its positives removed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::NgramTokenizer;
use tantivy::{Index, IndexReader, TantivyDocument, doc};

use crate::embed::{Embedder, TextKind};

#[derive(Debug, Deserialize)]
pub struct LaneQuery {
    pub id: String,
    pub text: String,
    pub split: String,
}

#[derive(Debug, Deserialize)]
pub struct Passage {
    pub docid: String,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct Qrel {
    query: String,
    docid: String,
    relevance: u8,
}

#[derive(Debug, Deserialize)]
struct LaneFile {
    queries: Vec<LaneQuery>,
    passages: Vec<Passage>,
    qrels: Vec<Qrel>,
}

pub struct Lane {
    pub queries: Vec<LaneQuery>,
    pub passages: Vec<Passage>,
    /// Positive passage positions per query.
    pub relevant: BTreeMap<String, BTreeSet<usize>>,
}

pub fn load(path: &Path) -> Result<Lane, String> {
    let file: LaneFile = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let position: BTreeMap<&str, usize> = file
        .passages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.docid.as_str(), i))
        .collect();
    let mut relevant: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for qrel in &file.qrels {
        if qrel.relevance > 0 {
            let at = *position.get(qrel.docid.as_str()).ok_or("qrel passage missing")?;
            relevant.entry(qrel.query.clone()).or_default().insert(at);
        }
    }
    Ok(Lane {
        queries: file.queries,
        passages: file.passages,
        relevant,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexicalMode {
    Production,
    Bm25,
    Bigram,
}

pub struct Lexical {
    reader: IndexReader,
    index: Index,
    fields: [tantivy::schema::Field; 4],
}

impl Lexical {
    pub fn build(passages: &[Passage]) -> Result<Self, String> {
        let mut schema = Schema::builder();
        let bigram = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("bigram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let fields = [
            schema.add_text_field("title", tantivy::schema::TEXT),
            schema.add_text_field("text", tantivy::schema::TEXT),
            schema.add_text_field("title_bigram", bigram.clone()),
            schema.add_text_field("text_bigram", bigram),
        ];
        let position = schema.add_u64_field("position", STORED);
        let index = Index::create_in_ram(schema.build());
        index.tokenizers().register(
            "bigram",
            NgramTokenizer::new(2, 2, false).map_err(|e| e.to_string())?,
        );
        let mut writer = index.writer(64_000_000).map_err(|e| e.to_string())?;
        for (i, passage) in passages.iter().enumerate() {
            writer
                .add_document(doc!(
                    fields[0] => passage.title.as_str(),
                    fields[1] => passage.text.as_str(),
                    fields[2] => passage.title.as_str(),
                    fields[3] => passage.text.as_str(),
                    position => i as u64,
                ))
                .map_err(|e| e.to_string())?;
        }
        writer.commit().map_err(|e| e.to_string())?;
        let reader = index.reader().map_err(|e| e.to_string())?;
        Ok(Self {
            reader,
            index,
            fields,
        })
    }

    pub fn search(
        &self,
        text: &str,
        mode: LexicalMode,
        k: usize,
        excluded: &BTreeSet<usize>,
    ) -> Result<Vec<usize>, String> {
        let (fields, query) = match mode {
            LexicalMode::Production => (
                vec![self.fields[0], self.fields[1]],
                format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")),
            ),
            LexicalMode::Bm25 => (vec![self.fields[0], self.fields[1]], escape(text)),
            LexicalMode::Bigram => (vec![self.fields[2], self.fields[3]], escape(text)),
        };
        let parser = QueryParser::for_index(&self.index, fields);
        let Ok(query) = parser.parse_query(&query) else {
            return Ok(vec![]);
        };
        let searcher = self.reader.searcher();
        let position = searcher.schema().get_field("position").map_err(|e| e.to_string())?;
        let top = searcher
            .search(&query, &TopDocs::with_limit(k + excluded.len()).order_by_score())
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for (_, address) in top {
            let document: TantivyDocument = searcher.doc(address).map_err(|e| e.to_string())?;
            let at = document
                .get_first(position)
                .and_then(|value| value.as_u64())
                .ok_or("position")? as usize;
            if !excluded.contains(&at) {
                out.push(at);
            }
            if out.len() == k {
                break;
            }
        }
        Ok(out)
    }
}

/// Every query character that the query grammar would interpret is a space.
fn escape(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

pub struct Dense {
    vectors: Vec<Vec<f32>>,
}

impl Dense {
    pub fn build(embedder: &Embedder, passages: &[Passage]) -> Result<Self, String> {
        let texts: Vec<String> = passages
            .iter()
            .map(|p| format!("{} {}", p.title, p.text))
            .collect();
        // Length-sorted batches pad less; results are placed back by position.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].chars().count());
        let mut vectors = vec![Vec::new(); texts.len()];
        for (n, chunk) in order.chunks(16).enumerate() {
            let refs: Vec<&str> = chunk.iter().map(|&i| texts[i].as_str()).collect();
            for (&i, vector) in chunk.iter().zip(embedder.embed(&refs, TextKind::Passage)?.normalized) {
                vectors[i] = vector;
            }
            if n % 10 == 0 {
                eprintln!("embedded {} / {}", (n + 1) * 16, texts.len());
            }
        }
        Ok(Self { vectors })
    }

    /// Positions with cosine ≥ `tau`, best first, ties by position.
    pub fn search(&self, query: &[f32], tau: f32, k: usize, excluded: &BTreeSet<usize>) -> Vec<usize> {
        let mut scored: Vec<(f32, usize)> = self
            .vectors
            .iter()
            .enumerate()
            .filter(|(i, _)| !excluded.contains(i))
            .map(|(i, v)| (v.iter().zip(query).map(|(a, b)| a * b).sum::<f32>(), i))
            .filter(|(score, _)| *score >= tau)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(k).map(|(_, i)| i).collect()
    }
}

/// S1 PriorityConcat of stage lists, each passage once, cut at `k`.
pub fn concat(stages: &[&[usize]], k: usize) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for stage in stages {
        for at in *stage {
            if seen.insert(*at) {
                out.push(*at);
            }
        }
    }
    out.truncate(k);
    out
}

pub fn ndcg_at_10(ranked: &[usize], relevant: &BTreeSet<usize>) -> f64 {
    let dcg: f64 = ranked
        .iter()
        .take(10)
        .enumerate()
        .filter(|(_, at)| relevant.contains(at))
        .map(|(i, _)| 1.0 / ((i + 2) as f64).log2())
        .sum();
    let ideal: f64 = (0..relevant.len().min(10))
        .map(|i| 1.0 / ((i + 2) as f64).log2())
        .sum();
    if ideal == 0.0 { 0.0 } else { dcg / ideal }
}

pub fn recall_at_10(ranked: &[usize], relevant: &BTreeSet<usize>) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    ranked.iter().take(10).filter(|at| relevant.contains(at)).count() as f64
        / relevant.len() as f64
}

/// Paired bootstrap of mean(differences): (mean, lower 2.5%, upper 97.5%).
pub fn bootstrap(differences: &[f64], rounds: usize, seed: u64) -> (f64, f64, f64) {
    let n = differences.len();
    let mean = differences.iter().sum::<f64>() / n as f64;
    let mut state = seed;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut means: Vec<f64> = (0..rounds)
        .map(|_| (0..n).map(|_| differences[(next() % n as u64) as usize]).sum::<f64>() / n as f64)
        .collect();
    means.sort_by(f64::total_cmp);
    (
        mean,
        means[(rounds as f64 * 0.025) as usize],
        means[(rounds as f64 * 0.975) as usize],
    )
}
