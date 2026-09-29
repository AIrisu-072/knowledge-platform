use std::collections::BTreeMap;

use search_application::error::SearchError;
use search_application::ports::{BoxFuture, LexicalQuery, LexicalRetrieverPort};
use search_core::discovery::{CandidateIdentityClass, DiscoveryRequest, FederatedCandidate};
use search_core::projection::ProjectionGenerationKey;
use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, QueryParser, TermQuery, TermSetQuery};
use tantivy::schema::{IndexRecordOption, TantivyDocument, Value};

use crate::TantivyLexicalIndex;
use crate::schema::{kind_token, normalize_exact};

const MAX_QUERY_LIMIT: usize = 256;
const OVERSAMPLE_FACTOR: usize = 4;

struct RankedMatch {
    id: String,
    signal: &'static str,
    tier: usize,
    score: f32,
}

impl LexicalRetrieverPort for TantivyLexicalIndex {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move {
            let segment = self
                .generation(generation)
                .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
            let text = query.text.trim();
            if text.is_empty() || query.limit == 0 || query.limit > MAX_QUERY_LIMIT {
                return Err(SearchError::InvalidRequest(
                    "lexical text must be nonempty and limit must be 1..=256".into(),
                ));
            }
            let searcher = segment.reader.searcher();
            if searcher.num_docs() == 0 {
                return Ok(Vec::new());
            }
            // The Phase B-qualified query treats user text as a literal phrase.
            let literal = format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
            let normalized = normalize_exact(text);
            // Each tier has a bounded window, independent of Source cardinality.
            // Dedup across tiers can underfill a window; a later Discovery
            // expansion can request a larger one when evidence is insufficient.
            let window = query.limit * OVERSAMPLE_FACTOR;
            let mut best_matches: BTreeMap<String, RankedMatch> = BTreeMap::new();
            for (tier, (signal, field, exact)) in segment.fields.ranked().into_iter().enumerate() {
                let text_query: Box<dyn Query> = if exact {
                    Box::new(TermQuery::new(
                        Term::from_field_text(field, &normalized),
                        IndexRecordOption::Basic,
                    ))
                } else {
                    let parser = QueryParser::for_index(&segment.index, vec![field]);
                    Box::new(
                        parser
                            .parse_query(&literal)
                            .map_err(|error| SearchError::InvalidRequest(error.to_string()))?,
                    )
                };
                let filtered: Box<dyn Query> = if request.need.required_resource_types.is_empty() {
                    text_query
                } else {
                    let terms =
                        request.need.required_resource_types.iter().map(|kind| {
                            Term::from_field_text(segment.fields.kind, kind_token(*kind))
                        });
                    Box::new(BooleanQuery::intersection(vec![
                        text_query,
                        Box::new(TermSetQuery::new(terms)),
                    ]))
                };
                let matches = searcher
                    .search(
                        filtered.as_ref(),
                        &TopDocs::with_limit(window).order_by_score(),
                    )
                    .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
                for (score, address) in matches {
                    let document = searcher
                        .doc::<TantivyDocument>(address)
                        .map_err(|error| SearchError::OperationFailed(error.to_string()))?;
                    let id = document
                        .get_first(segment.fields.resource_ref)
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            SearchError::OperationFailed(
                                "lexical document has no resource ref".into(),
                            )
                        })?;
                    let metadata = segment.documents.get(id).ok_or_else(|| {
                        SearchError::OperationFailed("lexical document metadata is missing".into())
                    })?;
                    if !request.need.required_resource_types.is_empty()
                        && !request
                            .need
                            .required_resource_types
                            .contains(&metadata.kind)
                    {
                        continue;
                    }
                    let ranked = RankedMatch {
                        id: id.to_owned(),
                        signal,
                        tier,
                        score,
                    };
                    match best_matches.get(id) {
                        Some(existing) if existing.tier <= tier => {}
                        _ => {
                            best_matches.insert(id.to_owned(), ranked);
                        }
                    }
                }
            }
            let mut ranked: Vec<_> = best_matches.into_values().collect();
            ranked.sort_by(|left, right| {
                left.tier
                    .cmp(&right.tier)
                    .then_with(|| right.score.total_cmp(&left.score))
                    .then_with(|| left.id.cmp(&right.id))
            });
            let mut candidates = Vec::new();
            for hit in ranked.into_iter().take(query.limit) {
                let metadata = segment.documents.get(&hit.id).ok_or_else(|| {
                    SearchError::OperationFailed("lexical document metadata is missing".into())
                })?;
                let mut candidate = FederatedCandidate::new(
                    format!(
                        "{}:{}",
                        generation.source_id.as_uuid(),
                        metadata.resource_ref.as_uuid()
                    ),
                    CandidateIdentityClass::DurableResource,
                    generation.source_id,
                    "lexical",
                );
                candidate.resource_ref = Some(metadata.resource_ref);
                candidate.locator = metadata.locator.clone();
                candidate.provenance = metadata.provenance.clone();
                candidate.matched_signals.push(hit.signal.into());
                candidate.retrieval_trace_ref = Some(format!(
                    "{}:{}",
                    generation.source_id.as_uuid(),
                    generation.generation_id.as_uuid()
                ));
                candidates.push(candidate);
            }
            Ok(candidates)
        })
    }
}
