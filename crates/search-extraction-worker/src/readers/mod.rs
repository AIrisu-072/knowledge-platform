//! Format readers for the sealed one-shot Search extraction worker (P1-I03–I05).
//!
//! A reader walks the whole reader-visible scope of one raw item before it
//! returns. Content it cannot locate fails closed as a typed `Unsupported` or
//! permanent failure; locators are never reconstructed from flattened text.

mod archive;
mod csv;
mod docx;
mod html;
mod ooxml;
mod pdf;
mod pptx;
mod spreadsheet;
mod text;

use search_core::knowledge_unit::{
    ArchiveProfilePlan, BudgetKey, FormatId, FormatSettings, NativeLocator, UnitKind,
    normalize_unit_text,
};
use search_extraction_core::{
    BodyCoverage, BudgetMeter, CoverageReason, NativeOmission, PermanentFailureCode, ReaderFailure,
    WorkerFragment, WorkerOperation, WorkerReport, WorkerRequest, WorkerResponse, encode_response,
};

pub use pdf::{PdfiumUnavailable, warm_up_pdfium};

pub(crate) type ReadResult<T> = Result<T, ReaderFailure>;

pub(crate) const fn unsupported(reason: CoverageReason) -> ReaderFailure {
    ReaderFailure::Unsupported(reason)
}

pub(crate) const fn structure() -> ReaderFailure {
    unsupported(CoverageReason::UnsupportedStructure)
}

pub(crate) const fn resource_limit() -> ReaderFailure {
    unsupported(CoverageReason::ResourceLimit)
}

pub(crate) const fn corrupt() -> ReaderFailure {
    ReaderFailure::Permanent(PermanentFailureCode::CorruptDocument)
}

pub(crate) const fn malformed_archive() -> ReaderFailure {
    ReaderFailure::Permanent(PermanentFailureCode::MalformedArchive)
}

pub(crate) const fn extraction_failed() -> ReaderFailure {
    ReaderFailure::Permanent(PermanentFailureCode::TextExtractionFailed)
}

/// One located fragment before ordinals are assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unit {
    pub kind: UnitKind,
    pub text: String,
    pub locator: NativeLocator,
}

/// Reader-visible body of one raw item: located units, known omissions and the
/// number of scope items that were traversed.
#[derive(Debug, Default)]
pub(crate) struct Body {
    pub units: Vec<Unit>,
    pub reasons: Vec<CoverageReason>,
    pub omissions: Vec<NativeOmission>,
    pub scope_items: u32,
}

impl Body {
    /// Normalize with `nfc-lf-v1` and charge the unit budgets. Empty text is
    /// part of the traversed scope but produces no unit.
    pub(crate) fn unit(
        &mut self,
        meter: &mut BudgetMeter,
        kind: UnitKind,
        raw_text: &str,
        locator: NativeLocator,
    ) -> ReadResult<()> {
        let text = normalize_unit_text(raw_text);
        if text.is_empty() {
            return Ok(());
        }
        meter.charge(BudgetKey::Units, 1)?;
        meter.observe_peak(BudgetKey::UnitUtf8Bytes, text.len() as u64)?;
        self.units.push(Unit {
            kind,
            text,
            locator,
        });
        Ok(())
    }

    pub(crate) fn visit(&mut self, items: usize) -> ReadResult<()> {
        let items = u32::try_from(items).map_err(|_| resource_limit())?;
        self.scope_items = self
            .scope_items
            .checked_add(items)
            .ok_or_else(resource_limit)?;
        Ok(())
    }

    /// Record a physically located omission. Only a located omission may turn
    /// coverage into `Partial`.
    pub(crate) fn omit(
        &mut self,
        package_path: Option<String>,
        physical_child_path: Vec<u32>,
        reason: CoverageReason,
    ) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
        self.omissions.push(NativeOmission {
            package_path,
            physical_child_path,
            reason,
        });
    }

    /// Completed traversal → coverage. Omissions without any located unit are
    /// `Unsupported`, never a `Partial` that could later support absence.
    fn into_report(self) -> ReadResult<WorkerReport> {
        let coverage = if self.reasons.is_empty() {
            if !self.omissions.is_empty() {
                return Err(structure());
            }
            BodyCoverage::Supported
        } else if self.units.is_empty() || self.omissions.is_empty() {
            return Err(unsupported(self.reasons[0]));
        } else {
            BodyCoverage::Partial {
                reasons: self.reasons,
            }
        };
        let fragments = self
            .units
            .into_iter()
            .enumerate()
            .map(|(ordinal, unit)| {
                Ok(WorkerFragment {
                    ordinal: u32::try_from(ordinal).map_err(|_| resource_limit())?,
                    parent_ordinal: None,
                    kind: unit.kind,
                    text: unit.text,
                    locator: unit.locator,
                })
            })
            .collect::<ReadResult<Vec<_>>>()?;
        Ok(WorkerReport {
            coverage,
            fragments,
            reader_use: Vec::new(),
            scope_items: self.scope_items,
            known_omissions: self.omissions,
            traversal_complete: true,
        })
    }
}

/// Read one non-container format with its registered settings.
pub(crate) fn read_leaf(
    format: FormatId,
    settings: &FormatSettings,
    raw: &[u8],
    meter: &mut BudgetMeter,
    zip_depth: u64,
) -> ReadResult<Body> {
    match (format, settings) {
        (FormatId::Text, FormatSettings::Text { charset }) => text::read(raw, charset, meter),
        (
            FormatId::Csv,
            FormatSettings::Csv {
                charset,
                delimiter,
                quote,
            },
        ) => csv::read(raw, charset, *delimiter, *quote, meter),
        (FormatId::Html, FormatSettings::None) => html::read(raw, meter),
        (FormatId::Docx, FormatSettings::None) => docx::read(raw, meter, zip_depth + 1),
        (FormatId::Xlsx, FormatSettings::None) => {
            spreadsheet::read(raw, false, meter, zip_depth + 1)
        }
        (FormatId::Xlsm, FormatSettings::None) => {
            spreadsheet::read(raw, true, meter, zip_depth + 1)
        }
        (FormatId::Pptx, FormatSettings::None) => pptx::read(raw, meter, zip_depth + 1),
        (FormatId::Pdf, FormatSettings::None) => pdf::read(raw, meter),
        _ => Err(unsupported(CoverageReason::UnsupportedFormat)),
    }
}

/// Execute one decoded request against already verified raw bytes. The caller
/// has checked the raw binding, profile identity and native pins and has
/// sealed the process before parsing starts.
pub fn respond(
    request: &WorkerRequest,
    raw: &[u8],
    plan: Option<&ArchiveProfilePlan>,
) -> WorkerResponse {
    match report(request, raw, plan) {
        Ok(report) => WorkerResponse::Report(report),
        Err(failure) => WorkerResponse::Failure(failure),
    }
}

fn report(
    request: &WorkerRequest,
    raw: &[u8],
    plan: Option<&ArchiveProfilePlan>,
) -> ReadResult<WorkerReport> {
    let mut meter = BudgetMeter::new(request.budgets.clone()).map_err(|_| extraction_failed())?;
    meter.charge_input_once(raw.len() as u64)?;
    let report = match (request.format, plan) {
        (FormatId::Zip, Some(plan)) => archive::read(raw, plan, &mut meter)?,
        (FormatId::Zip, None) | (_, Some(_)) => return Err(extraction_failed()),
        (format, None) => {
            let definition = search_core::knowledge_unit::ExtractionProfileDefinitionV1::decode(
                &request.profile_bytes,
            )
            .map_err(|_| extraction_failed())?;
            if definition.format != format {
                return Err(extraction_failed());
            }
            read_leaf(format, &definition.format_settings, raw, &mut meter, 0)?.into_report()?
        }
    };
    let report = match &request.operation {
        WorkerOperation::Extract => report,
        WorkerOperation::ResolveLocators(locators) => resolve(report, locators)?,
    };
    let encoded = encode_response(&WorkerResponse::Report(report.clone()))
        .map_err(|_| ReaderFailure::Permanent(PermanentFailureCode::WorkerOutputLimit))?;
    if encoded.len() as u64 > request.budgets.get(BudgetKey::WorkerOutputBytes) {
        return Err(ReaderFailure::Permanent(
            PermanentFailureCode::WorkerOutputLimit,
        ));
    }
    Ok(report)
}

/// Re-read the item and return exactly the requested locators in request order.
/// Coverage and omission witnesses stay those of the full traversal.
fn resolve(report: WorkerReport, locators: &[NativeLocator]) -> ReadResult<WorkerReport> {
    let mut fragments = Vec::with_capacity(locators.len());
    for (ordinal, locator) in locators.iter().enumerate() {
        let found = report
            .fragments
            .iter()
            .find(|fragment| &fragment.locator == locator)
            .ok_or_else(extraction_failed)?;
        if fragments
            .iter()
            .any(|existing: &WorkerFragment| &existing.locator == locator)
        {
            return Err(extraction_failed());
        }
        fragments.push(WorkerFragment {
            ordinal: u32::try_from(ordinal).map_err(|_| resource_limit())?,
            parent_ordinal: None,
            kind: found.kind,
            text: found.text.clone(),
            locator: found.locator.clone(),
        });
    }
    Ok(WorkerReport {
        fragments,
        ..report
    })
}

/// In-process extraction used by tests and by the qualification harness. The
/// production binary uses [`respond`] only after the sandbox is sealed.
pub fn extract_for_test(
    format: FormatId,
    settings: &FormatSettings,
    raw: &[u8],
    meter: &mut BudgetMeter,
) -> Result<WorkerReport, ReaderFailure> {
    meter.charge_input_once(raw.len() as u64)?;
    read_leaf(format, settings, raw, meter, 0)?.into_report()
}

/// In-process composite extraction used by tests.
pub fn extract_archive_for_test(
    raw: &[u8],
    plan: &ArchiveProfilePlan,
    meter: &mut BudgetMeter,
) -> Result<WorkerReport, ReaderFailure> {
    meter.charge_input_once(raw.len() as u64)?;
    archive::read(raw, plan, meter)
}
