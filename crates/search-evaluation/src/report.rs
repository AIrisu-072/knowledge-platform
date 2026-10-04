//! Low-cardinality evaluation counts. Reports never contain Source or Resource content.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::scenario::ScenarioCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClass {
    SourceKnowledgeAbsent,
    SourceRoutingMiss,
    RetrievalMiss,
    GraphPathMiss,
    ApplicabilityError,
    EvidenceLocatorError,
    SecurityExposure,
    GapReportingError,
    CompletionError,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RoutingMetrics {
    pub required_sources: usize,
    pub planned_sources: usize,
    pub misses: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RetrievalMetrics {
    pub relevant_resources: usize,
    pub found: usize,
    pub misses: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct GraphMetrics {
    pub required_relations: usize,
    pub found: usize,
    pub misses: usize,
    pub invalid_paths: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ApplicabilityMetrics {
    pub eligible_retrieved: usize,
    pub qualified_eligible: usize,
    pub false_rejects: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct EvidenceMetrics {
    pub required_claims: usize,
    pub located: usize,
    pub locator_errors: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CoverageMetrics {
    pub known_absent_cases: usize,
    pub unverified_cases: usize,
    pub expected_gaps: usize,
    pub observed_expected_gaps: usize,
    pub missing_expected_gaps: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CompletionMetrics {
    pub expected_complete_cases: usize,
    pub observed_complete_cases: usize,
    pub correct_cases: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StageMetrics {
    pub routing: RoutingMetrics,
    pub retrieval: RetrievalMetrics,
    pub graph: GraphMetrics,
    pub applicability: ApplicabilityMetrics,
    pub evidence: EvidenceMetrics,
    pub coverage: CoverageMetrics,
    pub completion: CompletionMetrics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SafetyMetrics {
    pub false_composite_paths: usize,
    pub hard_false_accepts: usize,
    pub unauthorized_exposures: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioOutcome {
    pub category: ScenarioCategory,
    pub failure: Option<FailureClass>,
    pub metrics: StageMetrics,
    pub safety: SafetyMetrics,
}

/// Aggregate only bounded category names, reason codes, and counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvaluationReport {
    pub schema_version: u8,
    pub scenario_count: usize,
    pub categories: BTreeMap<ScenarioCategory, usize>,
    pub failures: BTreeMap<FailureClass, usize>,
    pub stage_metrics: StageMetrics,
    pub safety: SafetyMetrics,
}

impl EvaluationReport {
    pub fn from_outcomes(outcomes: &[ScenarioOutcome]) -> Self {
        let mut report = Self {
            schema_version: 1,
            scenario_count: outcomes.len(),
            categories: BTreeMap::new(),
            failures: BTreeMap::new(),
            stage_metrics: StageMetrics::default(),
            safety: SafetyMetrics::default(),
        };
        for outcome in outcomes {
            *report.categories.entry(outcome.category).or_default() += 1;
            if let Some(failure) = outcome.failure {
                *report.failures.entry(failure).or_default() += 1;
            }
            let src = &outcome.metrics;
            let dst = &mut report.stage_metrics;
            dst.routing.required_sources += src.routing.required_sources;
            dst.routing.planned_sources += src.routing.planned_sources;
            dst.routing.misses += src.routing.misses;
            dst.retrieval.relevant_resources += src.retrieval.relevant_resources;
            dst.retrieval.found += src.retrieval.found;
            dst.retrieval.misses += src.retrieval.misses;
            dst.graph.required_relations += src.graph.required_relations;
            dst.graph.found += src.graph.found;
            dst.graph.misses += src.graph.misses;
            dst.graph.invalid_paths += src.graph.invalid_paths;
            dst.applicability.eligible_retrieved += src.applicability.eligible_retrieved;
            dst.applicability.qualified_eligible += src.applicability.qualified_eligible;
            dst.applicability.false_rejects += src.applicability.false_rejects;
            dst.evidence.required_claims += src.evidence.required_claims;
            dst.evidence.located += src.evidence.located;
            dst.evidence.locator_errors += src.evidence.locator_errors;
            dst.coverage.known_absent_cases += src.coverage.known_absent_cases;
            dst.coverage.unverified_cases += src.coverage.unverified_cases;
            dst.coverage.expected_gaps += src.coverage.expected_gaps;
            dst.coverage.observed_expected_gaps += src.coverage.observed_expected_gaps;
            dst.coverage.missing_expected_gaps += src.coverage.missing_expected_gaps;
            dst.completion.expected_complete_cases += src.completion.expected_complete_cases;
            dst.completion.observed_complete_cases += src.completion.observed_complete_cases;
            dst.completion.correct_cases += src.completion.correct_cases;
            report.safety.false_composite_paths += outcome.safety.false_composite_paths;
            report.safety.hard_false_accepts += outcome.safety.hard_false_accepts;
            report.safety.unauthorized_exposures += outcome.safety.unauthorized_exposures;
        }
        report
    }

    pub fn failure_count(&self, failure: FailureClass) -> usize {
        self.failures.get(&failure).copied().unwrap_or_default()
    }
}
