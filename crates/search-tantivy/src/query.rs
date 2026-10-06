use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use search_application::body_ports::{KnowledgeUnitHitRef, LexicalHit, LexicalRetrievalBatch};
use search_application::error::SearchError;
use search_application::ports::{BoxFuture, LexicalFieldScope, LexicalQuery, LexicalRetrieverPort};
use search_core::discovery::{CandidateIdentityClass, DiscoveryRequest, FederatedCandidate};
use search_core::id::ResourceId;
use search_core::knowledge_unit::{KnowledgeUnit, TextSpan, normalize_unit_text};
use search_core::projection::ProjectionGenerationKey;
use tantivy::Term;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermQuery, TermSetQuery};
use tantivy::schema::{IndexRecordOption, TantivyDocument, Value};
use tantivy::tokenizer::TokenStream;

use crate::TantivyLexicalIndex;
use crate::body::read_unit;
use crate::index::{DocumentMetadata, GenerationIndex};
use crate::schema::{kind_token, normalize_exact};

const MAX_QUERY_LIMIT: usize = 256;
const OVERSAMPLE_FACTOR: usize = 4;
/// BodyOnly refill doubles the Unit window up to `limit * 64`, never past this.
const MAX_REFILL_FACTOR: usize = 16;
const MAX_BODY_WINDOW: usize = 8192;

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

    fn retrieve_body<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, LexicalRetrievalBatch> {
        Box::pin(async move { retrieve_units(self, generation, request, query) })
    }
}

/// The parent's representative Unit. A literal span outranks a token-only
/// hit, then score, ordinal and UnitId decide deterministically.
struct Representative {
    literal: bool,
    score: f32,
    unit: KnowledgeUnit,
    span: Option<TextSpan>,
}

impl Representative {
    fn rank(&self, other: &Self) -> Ordering {
        other
            .literal
            .cmp(&self.literal)
            .then_with(|| other.score.total_cmp(&self.score))
            .then_with(|| self.unit.ordinal.cmp(&other.unit.ordinal))
            .then_with(|| self.unit.unit_id.cmp(&other.unit.unit_id))
    }
}

fn failed(error: impl ToString) -> SearchError {
    SearchError::OperationFailed(error.to_string())
}

/// BodyOnly reads only Unit documents. Resources are collapsed to one
/// representative Unit each and `limit` applies to unique parents; a hit gets a
/// `KnowledgeUnitHitRef` only when the normalized literal occurs in Unit.text.
fn retrieve_units(
    index: &TantivyLexicalIndex,
    generation: ProjectionGenerationKey,
    request: &DiscoveryRequest,
    query: &LexicalQuery,
) -> Result<LexicalRetrievalBatch, SearchError> {
    if query.field_scope != LexicalFieldScope::BodyOnly
        || query.text.trim().is_empty()
        || query.limit == 0
        || query.limit > MAX_QUERY_LIMIT
    {
        return Err(SearchError::InvalidRequest(
            "BodyOnly text must be nonempty and limit must be 1..=256".into(),
        ));
    }
    let segment = index.generation(generation).map_err(failed)?;
    let units = segment.units.as_ref().ok_or_else(|| {
        SearchError::SourceUnavailable("lexical generation is not body-ready".into())
    })?;
    let literal = normalize_unit_text(&query.text);
    let mut tokenizer = units
        .index
        .tokenizer_for_field(units.fields.body)
        .map_err(failed)?;
    if !tokenizer.token_stream(&literal).advance() {
        // No token can be searched, so the index cannot enumerate matches.
        return Ok(LexicalRetrievalBatch {
            hits: Vec::new(),
            exhausted_matching_units: false,
        });
    }
    let phrase = format!("\"{}\"", literal.replace('\\', "\\\\").replace('"', "\\\""));
    let mut parsed = QueryParser::for_index(&units.index, vec![units.fields.body])
        .parse_query(&phrase)
        .map_err(|error| SearchError::InvalidRequest(error.to_string()))?;
    let terms_tier = segment.tokenizer == crate::analyzer::CJK_BIGRAM_TOKENIZER;
    if terms_tier {
        // Bigram generations also match Units holding most of the query's
        // tokens. The literal phrase still ranks first (Representative), and
        // a hit's span is the longest part of the query its Unit contains.
        let mut tokens = BTreeSet::new();
        let mut stream = tokenizer.token_stream(&literal);
        while stream.advance() {
            tokens.insert(stream.token().text.clone());
        }
        if tokens.len() > 1 {
            let required = if tokens.len() <= 2 {
                tokens.len()
            } else {
                (tokens.len() * 3).div_ceil(5)
            };
            let clauses: Vec<(Occur, Box<dyn Query>)> = tokens
                .iter()
                .map(|token| {
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            Term::from_field_text(units.fields.body, token),
                            IndexRecordOption::WithFreqs,
                        )) as Box<dyn Query>,
                    )
                })
                .collect();
            parsed = Box::new(BooleanQuery::with_minimum_required_clauses(
                clauses, required,
            ));
        }
    }
    let searcher = units.reader.searcher();
    let initial = query.limit * OVERSAMPLE_FACTOR;
    let max_window = (initial * MAX_REFILL_FACTOR).clamp(initial, MAX_BODY_WINDOW.max(initial));
    let mut window = initial;
    loop {
        let (matches, total) = searcher
            .search(
                parsed.as_ref(),
                &(TopDocs::with_limit(window).order_by_score(), Count),
            )
            .map_err(failed)?;
        let examined_all = total <= window;
        let mut skipped = false;
        let mut best: BTreeMap<ResourceId, Representative> = BTreeMap::new();
        for (score, address) in matches {
            let document = searcher.doc::<TantivyDocument>(address).map_err(failed)?;
            let unit = read_unit(&document, units.fields).map_err(failed)?;
            let parent = unit.version.resource_id;
            let Some(metadata) = segment.documents.get(&parent.as_uuid().to_string()) else {
                // A Unit without its Resource document cannot be a candidate.
                skipped = true;
                continue;
            };
            if !request.need.required_resource_types.is_empty()
                && !request
                    .need
                    .required_resource_types
                    .contains(&metadata.kind)
            {
                continue;
            }
            let span = unit.text.find(&literal).and_then(|start| {
                let start = u32::try_from(start).ok()?;
                let end = start.checked_add(u32::try_from(literal.len()).ok()?)?;
                TextSpan::new(&unit.text, start, end).ok()
            });
            let candidate = Representative {
                literal: span.is_some(),
                score,
                unit,
                span,
            };
            match best.get(&parent) {
                Some(existing) if existing.rank(&candidate) != Ordering::Greater => {}
                _ => {
                    best.insert(parent, candidate);
                }
            }
        }
        if best.len() < query.limit && !examined_all && window < max_window {
            // Many Units of one parent filled the window: widen it, bounded.
            window = (window * 2).min(max_window);
            continue;
        }
        let mut ranked: Vec<_> = best.into_values().collect();
        ranked.sort_by(|left, right| {
            left.rank(right).then_with(|| {
                left.unit
                    .version
                    .resource_id
                    .cmp(&right.unit.version.resource_id)
            })
        });
        let truncated = ranked.len() > query.limit;
        ranked.truncate(query.limit);
        let mut hits = Vec::with_capacity(ranked.len());
        for mut representative in ranked {
            if terms_tier && representative.span.is_none() {
                representative.span = longest_query_part(&representative.unit.text, &literal);
            }
            hits.push(body_hit(generation, &segment, representative)?);
        }
        return Ok(LexicalRetrievalBatch {
            hits,
            exhausted_matching_units: examined_all && !truncated && !skipped,
        });
    }
}

/// The longest part of `query` (at least two characters, at most 64) that
/// occurs literally in `text`, as a span of `text`.
fn longest_query_part(text: &str, query: &str) -> Option<TextSpan> {
    let chars: Vec<(usize, char)> = query.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map_or(query.len(), |(offset, _)| *offset);
    let longest = chars.len().min(64);
    for length in (2..=longest).rev() {
        for start in 0..=chars.len() - length {
            let part = &query[chars[start].0..end_of(start + length)];
            if part.trim().chars().count() < 2 {
                continue;
            }
            if let Some(at) = text.find(part) {
                let from = u32::try_from(at).ok()?;
                let to = from.checked_add(u32::try_from(part.len()).ok()?)?;
                return TextSpan::new(text, from, to).ok();
            }
        }
    }
    None
}

fn body_hit(
    generation: ProjectionGenerationKey,
    segment: &GenerationIndex,
    representative: Representative,
) -> Result<LexicalHit, SearchError> {
    let unit = representative.unit;
    let parent = unit.version.resource_id;
    let metadata: &DocumentMetadata = segment
        .documents
        .get(&parent.as_uuid().to_string())
        .ok_or_else(|| failed("lexical document metadata is missing"))?;
    let mut candidate = FederatedCandidate::new(
        format!("{}:{}", generation.source_id.as_uuid(), parent.as_uuid()),
        CandidateIdentityClass::DurableResource,
        generation.source_id,
        "lexical",
    );
    candidate.resource_ref = Some(metadata.resource_ref);
    candidate.locator = metadata.locator.clone();
    candidate.provenance = metadata.provenance.clone();
    candidate.matched_signals.push("body".into());
    candidate.retrieval_trace_ref = Some(format!(
        "{}:{}",
        generation.source_id.as_uuid(),
        generation.generation_id.as_uuid()
    ));
    let unit_hit = match representative.span {
        Some(span) => {
            let locator = unit.locator.encode().map_err(failed)?;
            Some(KnowledgeUnitHitRef {
                generation,
                parent_resource: parent,
                authoritative_representation_ref: unit.provenance.authoritative_representation_ref,
                raw: unit.provenance.raw,
                profile: unit.provenance.profile,
                version: unit.version,
                part: unit.part,
                unit_id: unit.unit_id,
                excerpt: search_application::body_ports::excerpt_around(&unit.text, &span),
                span,
                text_sha256: unit.text_sha256,
                opaque_locator: locator.iter().map(|byte| format!("{byte:02x}")).collect(),
            })
        }
        None => None,
    };
    Ok(LexicalHit {
        candidate,
        unit_hit,
    })
}
