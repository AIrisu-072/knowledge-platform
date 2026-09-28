use std::collections::BTreeMap;
use std::fs;
use std::time::Instant;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lindera_analysis::tokenizer::Tokenizer as LinderaTokenizer;
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing, TextOptions,
    Value,
};
use tantivy::{Index, doc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzerKind {
    TantivyDefault,
    LinderaIpadic,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalResource {
    pub id: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub kind: String,
    pub audience: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalCase {
    pub id: String,
    pub query: String,
    pub expected_resource_ids: Vec<String>,
    pub forbidden_resource_ids: Vec<String>,
    pub required_kind: Option<String>,
    pub required_audience: Option<String>,
    pub mandatory: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexicalMeasurement {
    pub analyzer_id: String,
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub index_bytes: u64,
    pub build_ms: f64,
    pub query_p50_ms: f64,
    pub query_p95_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LexicalCaseResult {
    pub case_id: String,
    pub retrieved_ids: Vec<String>,
    pub qualified_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LexicalEvaluation {
    pub measurement: LexicalMeasurement,
    pub cases: Vec<LexicalCaseResult>,
}

pub fn evaluate_lexical(
    analyzer: AnalyzerKind,
    resources: &[LexicalResource],
    cases: &[LexicalCase],
) -> Result<LexicalEvaluation, Box<dyn std::error::Error>> {
    if resources.is_empty()
        || cases.is_empty()
        || cases
            .iter()
            .any(|case| case.expected_resource_ids.is_empty())
    {
        return Err("lexical corpus and expected resource ids must be nonempty".into());
    }
    let build_start = Instant::now();
    let lindera = match analyzer {
        AnalyzerKind::TantivyDefault => None,
        AnalyzerKind::LinderaIpadic => Some(LinderaTokenizer::new(Segmenter::new(
            Mode::Normal,
            load_dictionary("embedded://ipadic")?,
            None,
        ))),
    };
    let analyze = |text: &str| -> Result<String, Box<dyn std::error::Error>> {
        match &lindera {
            None => Ok(text.to_owned()),
            Some(tokenizer) => Ok(tokenizer
                .tokenize(text)?
                .iter()
                .map(|token| token.surface.as_ref())
                .collect::<Vec<&str>>()
                .join(" ")),
        }
    };
    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_text_field("id", STRING | STORED);
    let text_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let title_field = schema_builder.add_text_field("title", text_options.clone());
    let alias_field = schema_builder.add_text_field("aliases", text_options.clone());
    let summary_field = schema_builder.add_text_field("summary", text_options);
    let schema = schema_builder.build();
    let directory = tempfile::tempdir()?;
    let index = Index::create_in_dir(directory.path(), schema)?;
    let mut writer = index.writer(15_000_000)?;
    for resource in resources {
        let title = analyze(&resource.title)?;
        let summary = analyze(&resource.summary)?;
        let mut document = doc!(id_field => resource.id.as_str(), title_field => title.as_str(), summary_field => summary.as_str());
        for alias in &resource.aliases {
            document.add_text(alias_field, analyze(alias)?);
        }
        writer.add_document(document)?;
    }
    writer.commit()?;
    writer.wait_merging_threads()?;
    let reader = index.reader()?;
    reader.reload()?;
    let searcher = reader.searcher();
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
    let index_bytes = fs::read_dir(directory.path())?
        .map(|entry| {
            entry
                .and_then(|entry| entry.metadata())
                .map(|metadata| metadata.len())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum();
    let parser = QueryParser::for_index(&index, vec![title_field, alias_field, summary_field]);
    let by_id: BTreeMap<&str, &LexicalResource> = resources
        .iter()
        .map(|resource| (resource.id.as_str(), resource))
        .collect();
    let mut query_samples = Vec::new();
    let mut results = Vec::new();
    let mut found = 0usize;
    let mut expected_count = 0usize;
    let mut reciprocal_sum = 0.0;
    let mut ndcg_sum = 0.0;
    for case in cases {
        let mut retrieved_ids = Vec::new();
        for _ in 0..5 {
            let query_start = Instant::now();
            // Include analyzer work in latency; quoting treats the input as literal text.
            let literal = format!("\"{}\"", analyze(&case.query)?.replace('"', "\\\""));
            let query = parser.parse_query(&literal)?;
            let top_docs = searcher.search(&query, &TopDocs::with_limit(10).order_by_score())?;
            retrieved_ids = top_docs
                .into_iter()
                .map(|(_, address)| {
                    let document = searcher.doc::<TantivyDocument>(address)?;
                    document
                        .get_first(id_field)
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                        .ok_or_else(|| "indexed resource is missing id".into())
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            query_samples.push(query_start.elapsed().as_secs_f64() * 1000.0);
        }
        let qualified_ids = retrieved_ids
            .iter()
            .filter(|id| {
                by_id.get((*id).as_str()).is_some_and(|resource| {
                    case.required_kind
                        .as_deref()
                        .is_none_or(|required| resource.kind == required)
                        && case.required_audience.as_deref().is_none_or(|required| {
                            resource.audience == required || resource.audience == "any"
                        })
                })
            })
            .cloned()
            .collect();
        expected_count += case.expected_resource_ids.len();
        found += case
            .expected_resource_ids
            .iter()
            .filter(|id| retrieved_ids.contains(id))
            .count();
        let first_rank = retrieved_ids
            .iter()
            .position(|id| case.expected_resource_ids.contains(id));
        if let Some(rank) = first_rank {
            reciprocal_sum += 1.0 / (rank + 1) as f64;
            ndcg_sum += 1.0 / ((rank + 2) as f64).log2();
        }
        results.push(LexicalCaseResult {
            case_id: case.id.clone(),
            retrieved_ids,
            qualified_ids,
        });
    }
    query_samples.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        let index = ((query_samples.len() - 1) as f64 * fraction).ceil() as usize;
        query_samples[index]
    };
    let measurement = LexicalMeasurement {
        analyzer_id: match analyzer {
            AnalyzerKind::TantivyDefault => "tantivy-default-0.26.2".into(),
            AnalyzerKind::LinderaIpadic => "lindera-ipadic-6.2.0-tantivy-0.26.2".into(),
        },
        recall_at_10: found as f64 / expected_count as f64,
        mrr: reciprocal_sum / cases.len() as f64,
        ndcg_at_10: ndcg_sum / cases.len() as f64,
        index_bytes,
        build_ms,
        query_p50_ms: percentile(0.50),
        query_p95_ms: percentile(0.95),
    };
    Ok(LexicalEvaluation {
        measurement,
        cases: results,
    })
}
