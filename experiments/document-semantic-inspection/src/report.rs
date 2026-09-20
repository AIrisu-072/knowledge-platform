use crate::manifest::FixtureClass;
use crate::{ErrorCode, FormatId, PocError};
use serde::Serialize;
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
