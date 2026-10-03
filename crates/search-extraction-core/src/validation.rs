//! Trusted profile registration and pure validation of untrusted worker output.

use std::collections::BTreeSet;

use search_core::knowledge_unit::{
    ArchiveProfilePlan, BudgetKey, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    NativeLocator, UnitKind, normalize_unit_text, validate_archive_member,
};
use thiserror::Error;

use crate::budget::ExtractionBudgets;
use crate::coverage::{BodyCoverage, CoverageReason, PermanentFailureCode, RetryableFailureCode};
use crate::protocol::{
    WorkerFragment, WorkerOperation, WorkerReport, WorkerRequest, WorkerResponse, encode_response,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtractionError {
    #[error("extraction wire: {0}")]
    Wire(&'static str),
    #[error("extraction configuration: {0}")]
    Configuration(&'static str),
    #[error("extraction integrity: {0}")]
    Integrity(&'static str),
    #[error("extraction permanent worker failure: {0:?}")]
    Permanent(PermanentFailureCode),
    #[error("extraction retryable worker failure: {0:?}")]
    Retryable(RetryableFailureCode),
}

/// Construct only from the trusted host registry, never from worker bytes.
/// Canonical ID and all fifteen effective budget values are recomputed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredProfile {
    id: ExtractionProfileId,
    definition: ExtractionProfileDefinitionV1,
    parser_build_sha256: [u8; 32],
    native_binary_sha256: Option<[u8; 32]>,
    budgets: ExtractionBudgets,
    archive_plan: Option<ArchiveProfilePlan>,
    profile_bytes: Vec<u8>,
}

impl RegisteredProfile {
    pub fn register_definition(
        definition: ExtractionProfileDefinitionV1,
    ) -> Result<Self, ExtractionError> {
        if definition.format == FormatId::Zip {
            return Err(ExtractionError::Configuration(
                "ZIP requires composite profile",
            ));
        }
        let profile_bytes = definition
            .encode()
            .map_err(|_| ExtractionError::Configuration("profile definition"))?;
        let budgets = ExtractionBudgets::from_profile(&definition.limits)?;
        budgets.validate_for(definition.format)?;
        let id = ExtractionProfileId::for_definition(&definition)
            .map_err(|_| ExtractionError::Configuration("profile ID"))?;
        Ok(Self {
            id,
            parser_build_sha256: definition.parser_build_sha256,
            native_binary_sha256: definition.native_binary_sha256,
            definition,
            budgets,
            archive_plan: None,
            profile_bytes,
        })
    }

    pub fn register_archive(plan: ArchiveProfilePlan) -> Result<Self, ExtractionError> {
        let profile_bytes = plan
            .encode()
            .map_err(|_| ExtractionError::Configuration("archive plan"))?;
        let id = ExtractionProfileId::for_archive(&plan)
            .map_err(|_| ExtractionError::Configuration("archive profile ID"))?;
        let definition = plan
            .nodes
            .first()
            .ok_or(ExtractionError::Configuration("archive root"))?
            .definition
            .clone();
        let budgets = ExtractionBudgets::from_profile(&definition.limits)?;
        budgets.validate_for(FormatId::Zip)?;
        for node in &plan.nodes {
            ExtractionBudgets::from_profile(&node.definition.limits)?
                .validate_for(node.definition.format)?;
        }
        Ok(Self {
            id,
            parser_build_sha256: definition.parser_build_sha256,
            native_binary_sha256: definition.native_binary_sha256,
            definition,
            budgets,
            archive_plan: Some(plan),
            profile_bytes,
        })
    }

    pub fn id(&self) -> &ExtractionProfileId {
        &self.id
    }

    pub fn definition(&self) -> &ExtractionProfileDefinitionV1 {
        &self.definition
    }

    pub fn parser_build_sha256(&self) -> &[u8; 32] {
        &self.parser_build_sha256
    }

    pub fn native_binary_sha256(&self) -> Option<&[u8; 32]> {
        self.native_binary_sha256.as_ref()
    }

    pub fn budgets(&self) -> &ExtractionBudgets {
        &self.budgets
    }

    pub fn archive_plan(&self) -> Option<&ArchiveProfilePlan> {
        self.archive_plan.as_ref()
    }

    pub fn profile_bytes(&self) -> &[u8] {
        &self.profile_bytes
    }
}

/// Bind the worker request to an independently registered host profile.
pub fn validate_worker_request(
    request: &WorkerRequest,
    profile: &RegisteredProfile,
) -> Result<(), ExtractionError> {
    if request.format != profile.definition.format
        || request.profile != profile.id
        || request.profile_bytes != profile.profile_bytes
        || request.budgets != profile.budgets
    {
        return Err(ExtractionError::Configuration("request profile binding"));
    }
    request.budgets.validate_for(request.format)?;
    if request.expected_raw.size_bytes > request.budgets.get(BudgetKey::InputBytes)
        || !valid_media_type(&request.expected_raw.media_type)
    {
        return Err(ExtractionError::Integrity("raw binding"));
    }
    if let WorkerOperation::ResolveLocators(locators) = &request.operation
        && (locators.len() as u64 > request.budgets.get(BudgetKey::Units)
            || locators.iter().any(|locator| locator.encode().is_err()))
    {
        return Err(ExtractionError::Integrity("resolve locator"));
    }
    Ok(())
}

fn valid_media_type(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    let valid_token = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || b"!#$%&'*+-.^_`|~".contains(&byte)
            })
    };
    valid_token(kind) && valid_token(subtype)
}

fn compatible_kind(format: FormatId, locator: &NativeLocator, kind: UnitKind) -> bool {
    if let (FormatId::Docx, NativeLocator::Docx { steps }) = (format, locator) {
        return match kind {
            UnitKind::Heading => true,
            UnitKind::Paragraph => steps.len() == 1,
            UnitKind::TableCell => steps.len() > 1,
            _ => false,
        };
    }
    matches!(
        (format, locator, kind),
        (
            FormatId::Xlsx | FormatId::Xlsm,
            NativeLocator::Spreadsheet { .. },
            UnitKind::SpreadsheetCell
        ) | (
            FormatId::Pptx,
            NativeLocator::Pptx { .. },
            UnitKind::SlideText
        ) | (FormatId::Pdf, NativeLocator::Pdf { .. }, UnitKind::PdfText)
            | (
                FormatId::Text,
                NativeLocator::Text { .. },
                UnitKind::PlainText
            )
            | (FormatId::Csv, NativeLocator::Csv { .. }, UnitKind::CsvField)
            | (
                FormatId::Html,
                NativeLocator::Html { .. },
                UnitKind::HtmlText | UnitKind::Heading
            )
    )
}

fn validate_fragment(
    fragment: &WorkerFragment,
    index: usize,
    profile: &RegisteredProfile,
) -> Result<Vec<u8>, ExtractionError> {
    if fragment.ordinal as usize != index
        || fragment
            .parent_ordinal
            .is_some_and(|parent| parent >= fragment.ordinal)
        || fragment.text.is_empty()
        || normalize_unit_text(&fragment.text) != fragment.text
        || fragment.text.len() as u64 > profile.budgets.get(BudgetKey::UnitUtf8Bytes)
    {
        return Err(ExtractionError::Integrity("fragment ordinal/text"));
    }
    let locator_bytes = fragment
        .locator
        .encode()
        .map_err(|_| ExtractionError::Integrity("fragment locator"))?;
    match (&fragment.locator, profile.definition.format) {
        (NativeLocator::Archive { members, inner }, FormatId::Zip) => {
            let plan = profile
                .archive_plan
                .as_ref()
                .ok_or(ExtractionError::Integrity("archive plan"))?;
            let leaf = plan
                .nodes
                .iter()
                .find(|node| &node.members == members)
                .ok_or(ExtractionError::Integrity("archive leaf"))?;
            if !plan.used_leaf_chains.contains(members)
                || leaf.definition.format == FormatId::Zip
                || fragment.text.len() as u64 > leaf.definition.limits[&BudgetKey::UnitUtf8Bytes]
                || !compatible_kind(leaf.definition.format, inner, fragment.kind)
            {
                return Err(ExtractionError::Integrity("archive leaf kind"));
            }
        }
        (NativeLocator::Archive { .. }, _) | (_, FormatId::Zip) => {
            return Err(ExtractionError::Integrity("archive locator"));
        }
        (locator, format) if !compatible_kind(format, locator, fragment.kind) => {
            return Err(ExtractionError::Integrity("fragment kind"));
        }
        _ => {}
    }
    Ok(locator_bytes)
}

/// Verifies the pure syntax and coverage contract. The trusted Source host must still
/// re-read raw bytes and prove native locator/text round trips before constructing Units.
pub fn validate_worker_report(
    report: &WorkerReport,
    profile: &RegisteredProfile,
) -> Result<(), ExtractionError> {
    profile.budgets.validate_for(profile.definition.format)?;
    let serialized_size = encode_response(&WorkerResponse::Report(report.clone()))
        .map_err(|_| ExtractionError::Integrity("worker output encoding"))?
        .len();
    if serialized_size as u64 > profile.budgets.get(BudgetKey::WorkerOutputBytes) {
        return Err(ExtractionError::Integrity("worker output budget"));
    }
    if !report.traversal_complete {
        // A structured Unsupported failure terminates an item without claiming
        // a completed reader traversal. It can carry no positive or scope witness.
        if matches!(report.coverage, BodyCoverage::Unsupported { .. })
            && report.fragments.is_empty()
            && report.reader_use.is_empty()
            && report.scope_items == 0
            && report.known_omissions.is_empty()
        {
            return Ok(());
        }
        return Err(ExtractionError::Integrity("incomplete traversal"));
    }
    if report.fragments.len() as u64 > profile.budgets.get(BudgetKey::Units)
        || (report.scope_items == 0 && !report.fragments.is_empty())
    {
        return Err(ExtractionError::Integrity("scope/unit count"));
    }
    match &report.coverage {
        BodyCoverage::Supported => {
            if !report.known_omissions.is_empty() {
                return Err(ExtractionError::Integrity("supported omissions"));
            }
        }
        BodyCoverage::Partial { reasons } => {
            if reasons.is_empty()
                || report.fragments.is_empty()
                || report.known_omissions.is_empty()
                || report.scope_items == 0
            {
                return Err(ExtractionError::Integrity("partial witness"));
            }
            if reasons.contains(&CoverageReason::ResourceLimit) {
                return Err(ExtractionError::Integrity("partial resource limit"));
            }
            let declared: BTreeSet<_> = reasons.iter().copied().collect();
            let omitted: BTreeSet<_> = report
                .known_omissions
                .iter()
                .map(|omission| omission.reason)
                .collect();
            if declared.len() != reasons.len() || declared != omitted {
                return Err(ExtractionError::Integrity("partial reasons"));
            }
        }
        BodyCoverage::Unsupported { .. } => {
            if !report.fragments.is_empty() || !report.known_omissions.is_empty() {
                return Err(ExtractionError::Integrity("unsupported output"));
            }
        }
    }
    let mut seen_locators = BTreeSet::new();
    for (index, fragment) in report.fragments.iter().enumerate() {
        let locator = validate_fragment(fragment, index, profile)?;
        if !seen_locators.insert(locator) {
            return Err(ExtractionError::Integrity("duplicate locator"));
        }
    }
    let mut seen_omissions = BTreeSet::new();
    for omission in &report.known_omissions {
        if omission.physical_child_path.len() > 256
            || omission
                .package_path
                .as_ref()
                .is_some_and(|path| path.len() > 4_096 || validate_archive_member(path).is_err())
            || (omission.package_path.is_none() && omission.physical_child_path.is_empty())
        {
            return Err(ExtractionError::Integrity("omission scope"));
        }
        if !seen_omissions.insert((
            omission.package_path.as_deref(),
            omission.physical_child_path.as_slice(),
        )) {
            return Err(ExtractionError::Integrity("duplicate omission"));
        }
    }
    match &profile.archive_plan {
        Some(plan) if report.reader_use != plan.nodes => {
            return Err(ExtractionError::Integrity("archive reader-use"));
        }
        None if !report.reader_use.is_empty() => {
            return Err(ExtractionError::Integrity("nonarchive reader-use"));
        }
        _ => {}
    }
    Ok(())
}

/// Only validated reports are returned; interrupted Supported/Partial results and
/// unconverted failure responses cannot become completed reports. A terminal
/// Unsupported report has zero output and does not claim full traversal.
pub fn checked_completed_report<'a>(
    response: &'a WorkerResponse,
    profile: &RegisteredProfile,
) -> Result<&'a WorkerReport, ExtractionError> {
    let WorkerResponse::Report(report) = response else {
        return Err(ExtractionError::Integrity("no completed report"));
    };
    validate_worker_report(report, profile)?;
    Ok(report)
}

/// A structured ResourceLimit is an Unsupported item, not a partial success.
pub const fn resource_limit_failure() -> crate::protocol::ReaderFailure {
    crate::protocol::ReaderFailure::Unsupported(CoverageReason::ResourceLimit)
}
