use crate::manifest::FixtureClass;
use crate::{ErrorCode, FormatId, PocError};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseReport {
    pub id: String,
    pub class: FixtureClass,
    pub format: FormatId,
    pub verdict: CaseVerdict,
    pub semantic_fingerprint: Option<String>,
    pub error_code: Option<ErrorCode>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationReport {
    pub passed: bool,
    pub cases: Vec<CaseReport>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct GateCount {
    pub passed: usize,
    pub required: usize,
}

impl GateCount {
    pub fn complete(self) -> bool {
        self.passed == self.required
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FixtureGateCounts {
    pub semantic_change: GateCount,
    pub noise_invariance: GateCount,
    pub editorial: GateCount,
    pub fail_closed: GateCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalGateEvidence {
    pub supplemental_fixtures: BTreeMap<FormatId, FixtureGateCounts>,
    pub determinism: BTreeMap<FormatId, GateCount>,
    pub security_resource: BTreeMap<FormatId, bool>,
    pub license_dependency: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormatPromotionGate {
    pub format: FormatId,
    pub semantic_change: GateCount,
    pub noise_invariance: GateCount,
    pub editorial: GateCount,
    pub fail_closed: GateCount,
    pub determinism: GateCount,
    pub security_resource: bool,
    pub license_dependency: bool,
    pub promotion_eligible: bool,
}

pub fn aggregate_promotion_gates(
    report: &VerificationReport,
    external: &ExternalGateEvidence,
) -> BTreeMap<FormatId, FormatPromotionGate> {
    let formats: BTreeSet<FormatId> = report.cases.iter().map(|case| case.format).collect();
    let mut result = BTreeMap::new();

    for format in formats {
        let supplemental = external
            .supplemental_fixtures
            .get(&format)
            .copied()
            .unwrap_or_default();
        let mut semantic_change = supplemental.semantic_change;
        let mut noise_invariance = supplemental.noise_invariance;
        let mut editorial = supplemental.editorial;
        let mut fail_closed = supplemental.fail_closed;

        for case in report.cases.iter().filter(|case| case.format == format) {
            let gate = match case.class {
                FixtureClass::Semantic => Some(&mut semantic_change),
                FixtureClass::Noise => Some(&mut noise_invariance),
                FixtureClass::Editorial => Some(&mut editorial),
                FixtureClass::Hostile => Some(&mut fail_closed),
                FixtureClass::Base => None,
            };
            if let Some(gate) = gate {
                gate.required += 1;
                if case.verdict == CaseVerdict::Pass {
                    gate.passed += 1;
                }
            }
        }

        let determinism = external
            .determinism
            .get(&format)
            .copied()
            .unwrap_or_default();
        let security_resource = external
            .security_resource
            .get(&format)
            .copied()
            .unwrap_or(false);
        let license_dependency = external.license_dependency;

        let promotion_eligible = semantic_change.complete()
            && noise_invariance.complete()
            && editorial.complete()
            && fail_closed.complete()
            && determinism.complete()
            && security_resource
            && license_dependency;

        result.insert(
            format,
            FormatPromotionGate {
                format,
                semantic_change,
                noise_invariance,
                editorial,
                fail_closed,
                determinism,
                security_resource,
                license_dependency,
                promotion_eligible,
            },
        );
    }

    result
}

pub fn write_reports(report: &VerificationReport, output_dir: &Path) -> Result<(), PocError> {
    fs::create_dir_all(output_dir).map_err(|error| {
        PocError::SemanticExtractionFailed(format!(
            "cannot create report directory {}: {error}",
            output_dir.display()
        ))
    })?;

    let json = serde_json::to_string_pretty(report).map_err(|error| {
        PocError::InvalidWorkerResult(format!("cannot serialize report: {error}"))
    })?;
    fs::write(output_dir.join("report.json"), format!("{json}\n")).map_err(|error| {
        PocError::SemanticExtractionFailed(format!("cannot write JSON report: {error}"))
    })?;

    let mut markdown = String::from(
        "# Document Semantic Inspection PoC Report\n\n| Case | Class | Format | Verdict | Evidence |\n|---|---|---|---|---|\n",
    );
    for case in &report.cases {
        let evidence = case
            .semantic_fingerprint
            .as_deref()
            .map(|value| format!("sha256:{value}"))
            .or_else(|| case.error_code.map(|code| format!("error:{code:?}")))
            .unwrap_or_else(|| "none".into());

        markdown.push_str(&format!(
            "| {} | {:?} | {:?} | {:?} | {} |\n",
            case.id, case.class, case.format, case.verdict, evidence
        ));
    }
    markdown.push_str(&format!(
        "\nOverall: **{}**\n",
        if report.passed { "PASS" } else { "FAIL" }
    ));

    fs::write(output_dir.join("report.md"), markdown).map_err(|error| {
        PocError::SemanticExtractionFailed(format!("cannot write Markdown report: {error}"))
    })?;

    Ok(())
}
