//! Bounded binary `search-extraction:v1` wire. Source bytes travel out of band.

use search_core::knowledge_unit::{
    ArchiveReaderNode, BudgetKey, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    NativeLocator, RawBinding, UnitKind,
};

use crate::budget::{ExtractionBudgets, MAX_WORKER_OUTPUT_BYTES};
use crate::coverage::{
    BodyCoverage, CoverageReason, ItemOperationState, PermanentFailureCode, RetryableFailureCode,
};
use crate::validation::ExtractionError;

const MAGIC: &[u8] = b"search-extraction:v1\0";
const REQUEST_TAG: u8 = 1;
const REPORT_TAG: u8 = 2;
const FAILURE_TAG: u8 = 3;
const MAX_REQUEST_BYTES: usize = 16_777_216;
const MAX_COLLECTION_COUNT: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerOperation {
    Extract,
    ResolveLocators(Vec<NativeLocator>),
}

/// Contains only raw binding, profile and budgets. Source and actor identities stay on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRequest {
    pub operation: WorkerOperation,
    pub format: FormatId,
    pub profile: ExtractionProfileId,
    pub profile_bytes: Vec<u8>,
    pub expected_raw: RawBinding,
    pub budgets: ExtractionBudgets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFragment {
    pub ordinal: u32,
    pub parent_ordinal: Option<u32>,
    pub kind: UnitKind,
    pub text: String,
    pub locator: NativeLocator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOmission {
    pub package_path: Option<String>,
    pub physical_child_path: Vec<u32>,
    pub reason: CoverageReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerReport {
    pub coverage: BodyCoverage,
    pub fragments: Vec<WorkerFragment>,
    pub reader_use: Vec<ArchiveReaderNode>,
    pub scope_items: u32,
    pub known_omissions: Vec<NativeOmission>,
    /// Set only after the entire reader-visible scope was traversed. Interrupted work
    /// cannot become Supported or Partial, even if some fragments were emitted.
    pub traversal_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderFailure {
    Unsupported(CoverageReason),
    Permanent(PermanentFailureCode),
    Retryable(RetryableFailureCode),
}

impl ReaderFailure {
    /// The caller must still enforce that retryable states are never published.
    pub const fn item_outcome(self) -> (ItemOperationState, Option<BodyCoverage>) {
        match self {
            Self::Unsupported(reason) => (
                ItemOperationState::Completed,
                Some(BodyCoverage::Unsupported { reason }),
            ),
            Self::Permanent(code) => (ItemOperationState::FailedPermanent { code }, None),
            Self::Retryable(code) => (ItemOperationState::Retryable { code }, None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerResponse {
    Report(WorkerReport),
    Failure(ReaderFailure),
}

fn format_tag(format: FormatId) -> u8 {
    match format {
        FormatId::Docx => 1,
        FormatId::Xlsx => 2,
        FormatId::Xlsm => 3,
        FormatId::Pptx => 4,
        FormatId::Pdf => 5,
        FormatId::Text => 6,
        FormatId::Csv => 7,
        FormatId::Html => 8,
        FormatId::Zip => 9,
    }
}

fn format_from_tag(tag: u8) -> Result<FormatId, ExtractionError> {
    Ok(match tag {
        1 => FormatId::Docx,
        2 => FormatId::Xlsx,
        3 => FormatId::Xlsm,
        4 => FormatId::Pptx,
        5 => FormatId::Pdf,
        6 => FormatId::Text,
        7 => FormatId::Csv,
        8 => FormatId::Html,
        9 => FormatId::Zip,
        _ => return Err(ExtractionError::Wire("format tag")),
    })
}

fn kind_tag(kind: UnitKind) -> u8 {
    match kind {
        UnitKind::Heading => 1,
        UnitKind::Paragraph => 2,
        UnitKind::TableCell => 3,
        UnitKind::SpreadsheetCell => 4,
        UnitKind::SlideText => 5,
        UnitKind::PdfText => 6,
        UnitKind::PlainText => 7,
        UnitKind::CsvField => 8,
        UnitKind::HtmlText => 9,
    }
}

fn kind_from_tag(tag: u8) -> Result<UnitKind, ExtractionError> {
    Ok(match tag {
        1 => UnitKind::Heading,
        2 => UnitKind::Paragraph,
        3 => UnitKind::TableCell,
        4 => UnitKind::SpreadsheetCell,
        5 => UnitKind::SlideText,
        6 => UnitKind::PdfText,
        7 => UnitKind::PlainText,
        8 => UnitKind::CsvField,
        9 => UnitKind::HtmlText,
        _ => return Err(ExtractionError::Wire("unit kind tag")),
    })
}

fn frame(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ExtractionError> {
    let length = u32::try_from(bytes.len()).map_err(|_| ExtractionError::Wire("frame length"))?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn string(out: &mut Vec<u8>, value: &str) -> Result<(), ExtractionError> {
    frame(out, value.as_bytes())
}

fn count(out: &mut Vec<u8>, value: usize) -> Result<(), ExtractionError> {
    if value > MAX_COLLECTION_COUNT {
        return Err(ExtractionError::Wire("collection count"));
    }
    let value = u32::try_from(value).map_err(|_| ExtractionError::Wire("collection count"))?;
    out.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ExtractionError> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or(ExtractionError::Wire("length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ExtractionError::Wire("truncated frame"))?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, ExtractionError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ExtractionError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ExtractionError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn frame(&mut self) -> Result<&'a [u8], ExtractionError> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    fn string(&mut self) -> Result<String, ExtractionError> {
        Ok(std::str::from_utf8(self.frame()?)
            .map_err(|_| ExtractionError::Wire("UTF-8"))?
            .to_owned())
    }

    fn count(&mut self, min_item_bytes: usize) -> Result<usize, ExtractionError> {
        let n = self.u32()? as usize;
        if n > MAX_COLLECTION_COUNT || n > self.remaining() / min_item_bytes {
            return Err(ExtractionError::Wire("collection count"));
        }
        Ok(n)
    }

    fn finish(&self) -> Result<(), ExtractionError> {
        if self.remaining() != 0 {
            return Err(ExtractionError::Wire("trailing bytes"));
        }
        Ok(())
    }
}

fn message(tag: u8, body: Vec<u8>, cap: usize) -> Result<Vec<u8>, ExtractionError> {
    let total = MAGIC
        .len()
        .checked_add(5)
        .and_then(|head| head.checked_add(body.len()))
        .ok_or(ExtractionError::Wire("message length"))?;
    if total > cap {
        return Err(ExtractionError::Wire("message limit"));
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    out.push(tag);
    frame(&mut out, &body)?;
    Ok(out)
}

fn read_message(bytes: &[u8], cap: usize) -> Result<(u8, Cursor<'_>), ExtractionError> {
    if bytes.len() > cap {
        return Err(ExtractionError::Wire("message limit"));
    }
    let mut input = Cursor::new(bytes);
    if input.take(MAGIC.len())? != MAGIC {
        return Err(ExtractionError::Wire("protocol version"));
    }
    let tag = input.byte()?;
    let body = input.frame()?;
    input.finish()?;
    Ok((tag, Cursor::new(body)))
}

fn write_budgets(out: &mut Vec<u8>, budgets: &ExtractionBudgets) {
    out.push(15);
    for (index, key) in BudgetKey::ALL.into_iter().enumerate() {
        out.push((index + 1) as u8);
        out.extend_from_slice(&budgets.get(key).to_be_bytes());
    }
}

fn read_budgets(input: &mut Cursor<'_>) -> Result<ExtractionBudgets, ExtractionError> {
    if input.byte()? != 15 {
        return Err(ExtractionError::Wire("budget count"));
    }
    let mut limits = std::collections::BTreeMap::new();
    for (index, key) in BudgetKey::ALL.into_iter().enumerate() {
        if input.byte()? != (index + 1) as u8 {
            return Err(ExtractionError::Wire("budget tag/order"));
        }
        limits.insert(key, input.u64()?);
    }
    ExtractionBudgets::new(limits)
}

pub fn encode_request(request: &WorkerRequest) -> Result<Vec<u8>, ExtractionError> {
    request.budgets.validate_for(request.format)?;
    let mut out = Vec::new();
    match &request.operation {
        WorkerOperation::Extract => out.push(1),
        WorkerOperation::ResolveLocators(locators) => {
            out.push(2);
            count(&mut out, locators.len())?;
            for locator in locators {
                frame(
                    &mut out,
                    &locator
                        .encode()
                        .map_err(|_| ExtractionError::Wire("locator"))?,
                )?;
            }
        }
    }
    out.push(format_tag(request.format));
    string(&mut out, request.profile.as_str())?;
    frame(&mut out, &request.profile_bytes)?;
    out.extend_from_slice(&request.expected_raw.sha256);
    out.extend_from_slice(&request.expected_raw.size_bytes.to_be_bytes());
    string(&mut out, &request.expected_raw.media_type)?;
    write_budgets(&mut out, &request.budgets);
    message(REQUEST_TAG, out, MAX_REQUEST_BYTES)
}

pub fn decode_request(bytes: &[u8]) -> Result<WorkerRequest, ExtractionError> {
    let (tag, mut input) = read_message(bytes, MAX_REQUEST_BYTES)?;
    if tag != REQUEST_TAG {
        return Err(ExtractionError::Wire("request tag"));
    }
    let operation = match input.byte()? {
        1 => WorkerOperation::Extract,
        2 => {
            let n = input.count(4)?;
            let mut locators = Vec::with_capacity(n);
            for _ in 0..n {
                locators.push(
                    NativeLocator::decode(input.frame()?)
                        .map_err(|_| ExtractionError::Wire("locator"))?,
                );
            }
            WorkerOperation::ResolveLocators(locators)
        }
        _ => return Err(ExtractionError::Wire("operation tag")),
    };
    let format = format_from_tag(input.byte()?)?;
    let profile = ExtractionProfileId::parse(&input.string()?)
        .map_err(|_| ExtractionError::Wire("profile ID"))?;
    let profile_bytes = input.frame()?.to_vec();
    let sha256 = input.take(32)?.try_into().expect("32 bytes");
    let size_bytes = input.u64()?;
    let media_type = input.string()?;
    let budgets = read_budgets(&mut input)?;
    input.finish()?;
    budgets.validate_for(format)?;
    Ok(WorkerRequest {
        operation,
        format,
        profile,
        profile_bytes,
        expected_raw: RawBinding {
            sha256,
            size_bytes,
            media_type,
        },
        budgets,
    })
}

fn write_coverage(out: &mut Vec<u8>, coverage: &BodyCoverage) -> Result<(), ExtractionError> {
    match coverage {
        BodyCoverage::Supported => out.push(1),
        BodyCoverage::Partial { reasons } => {
            out.push(2);
            count(out, reasons.len())?;
            for reason in reasons {
                out.push(reason.tag());
            }
        }
        BodyCoverage::Unsupported { reason } => {
            out.extend_from_slice(&[3, reason.tag()]);
        }
    }
    Ok(())
}

fn read_reason(input: &mut Cursor<'_>) -> Result<CoverageReason, ExtractionError> {
    CoverageReason::from_tag(input.byte()?).ok_or(ExtractionError::Wire("coverage reason tag"))
}

fn read_coverage(input: &mut Cursor<'_>) -> Result<BodyCoverage, ExtractionError> {
    Ok(match input.byte()? {
        1 => BodyCoverage::Supported,
        2 => {
            let n = input.count(1)?;
            let mut reasons = Vec::with_capacity(n);
            for _ in 0..n {
                reasons.push(read_reason(input)?);
            }
            BodyCoverage::Partial { reasons }
        }
        3 => BodyCoverage::Unsupported {
            reason: read_reason(input)?,
        },
        _ => return Err(ExtractionError::Wire("coverage tag")),
    })
}

fn write_fragment(out: &mut Vec<u8>, fragment: &WorkerFragment) -> Result<(), ExtractionError> {
    out.extend_from_slice(&fragment.ordinal.to_be_bytes());
    match fragment.parent_ordinal {
        None => out.push(0),
        Some(parent) => {
            out.push(1);
            out.extend_from_slice(&parent.to_be_bytes());
        }
    }
    out.push(kind_tag(fragment.kind));
    string(out, &fragment.text)?;
    frame(
        out,
        &fragment
            .locator
            .encode()
            .map_err(|_| ExtractionError::Wire("locator"))?,
    )?;
    Ok(())
}

fn read_fragment(input: &mut Cursor<'_>) -> Result<WorkerFragment, ExtractionError> {
    let ordinal = input.u32()?;
    let parent_ordinal = match input.byte()? {
        0 => None,
        1 => Some(input.u32()?),
        _ => return Err(ExtractionError::Wire("parent tag")),
    };
    let kind = kind_from_tag(input.byte()?)?;
    let text = input.string()?;
    let locator =
        NativeLocator::decode(input.frame()?).map_err(|_| ExtractionError::Wire("locator"))?;
    Ok(WorkerFragment {
        ordinal,
        parent_ordinal,
        kind,
        text,
        locator,
    })
}

fn write_reader(out: &mut Vec<u8>, reader: &ArchiveReaderNode) -> Result<(), ExtractionError> {
    count(out, reader.members.len())?;
    for member in &reader.members {
        string(out, member)?;
    }
    string(out, &reader.parser_build_id)?;
    frame(
        out,
        &reader
            .definition
            .encode()
            .map_err(|_| ExtractionError::Wire("reader definition"))?,
    )?;
    Ok(())
}

fn read_reader(input: &mut Cursor<'_>) -> Result<ArchiveReaderNode, ExtractionError> {
    let n = input.count(4)?;
    let mut members = Vec::with_capacity(n);
    for _ in 0..n {
        members.push(input.string()?);
    }
    let parser_build_id = input.string()?;
    let definition = ExtractionProfileDefinitionV1::decode(input.frame()?)
        .map_err(|_| ExtractionError::Wire("reader definition"))?;
    Ok(ArchiveReaderNode {
        members,
        parser_build_id,
        definition,
    })
}

fn write_omission(out: &mut Vec<u8>, omission: &NativeOmission) -> Result<(), ExtractionError> {
    match &omission.package_path {
        None => out.push(0),
        Some(path) => {
            out.push(1);
            string(out, path)?;
        }
    }
    count(out, omission.physical_child_path.len())?;
    for child in &omission.physical_child_path {
        out.extend_from_slice(&child.to_be_bytes());
    }
    out.push(omission.reason.tag());
    Ok(())
}

fn read_omission(input: &mut Cursor<'_>) -> Result<NativeOmission, ExtractionError> {
    let package_path = match input.byte()? {
        0 => None,
        1 => Some(input.string()?),
        _ => return Err(ExtractionError::Wire("package path tag")),
    };
    let n = input.count(4)?;
    let mut physical_child_path = Vec::with_capacity(n);
    for _ in 0..n {
        physical_child_path.push(input.u32()?);
    }
    Ok(NativeOmission {
        package_path,
        physical_child_path,
        reason: read_reason(input)?,
    })
}

pub fn encode_response(response: &WorkerResponse) -> Result<Vec<u8>, ExtractionError> {
    match response {
        WorkerResponse::Report(report) => {
            let mut out = Vec::new();
            write_coverage(&mut out, &report.coverage)?;
            count(&mut out, report.fragments.len())?;
            for fragment in &report.fragments {
                write_fragment(&mut out, fragment)?;
            }
            count(&mut out, report.reader_use.len())?;
            for reader in &report.reader_use {
                write_reader(&mut out, reader)?;
            }
            out.extend_from_slice(&report.scope_items.to_be_bytes());
            count(&mut out, report.known_omissions.len())?;
            for omission in &report.known_omissions {
                write_omission(&mut out, omission)?;
            }
            out.push(u8::from(report.traversal_complete));
            message(REPORT_TAG, out, MAX_WORKER_OUTPUT_BYTES as usize)
        }
        WorkerResponse::Failure(failure) => {
            let out = match failure {
                ReaderFailure::Unsupported(reason) => vec![1, reason.tag()],
                ReaderFailure::Permanent(code) => vec![2, code.tag()],
                ReaderFailure::Retryable(code) => vec![3, code.tag()],
            };
            message(FAILURE_TAG, out, MAX_WORKER_OUTPUT_BYTES as usize)
        }
    }
}

pub fn decode_response(bytes: &[u8]) -> Result<WorkerResponse, ExtractionError> {
    let (tag, mut input) = read_message(bytes, MAX_WORKER_OUTPUT_BYTES as usize)?;
    let response = match tag {
        REPORT_TAG => {
            let coverage = read_coverage(&mut input)?;
            let n = input.count(10)?;
            let mut fragments = Vec::with_capacity(n);
            for _ in 0..n {
                fragments.push(read_fragment(&mut input)?);
            }
            let n = input.count(12)?;
            let mut reader_use = Vec::with_capacity(n);
            for _ in 0..n {
                reader_use.push(read_reader(&mut input)?);
            }
            let scope_items = input.u32()?;
            let n = input.count(6)?;
            let mut known_omissions = Vec::with_capacity(n);
            for _ in 0..n {
                known_omissions.push(read_omission(&mut input)?);
            }
            let traversal_complete = match input.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ExtractionError::Wire("traversal tag")),
            };
            WorkerResponse::Report(WorkerReport {
                coverage,
                fragments,
                reader_use,
                scope_items,
                known_omissions,
                traversal_complete,
            })
        }
        FAILURE_TAG => {
            let code = input.byte()?;
            let failure = match code {
                1 => ReaderFailure::Unsupported(read_reason(&mut input)?),
                2 => ReaderFailure::Permanent(
                    PermanentFailureCode::from_tag(input.byte()?)
                        .ok_or(ExtractionError::Wire("permanent failure tag"))?,
                ),
                3 => ReaderFailure::Retryable(
                    RetryableFailureCode::from_tag(input.byte()?)
                        .ok_or(ExtractionError::Wire("retryable failure tag"))?,
                ),
                _ => return Err(ExtractionError::Wire("failure tag")),
            };
            WorkerResponse::Failure(failure)
        }
        _ => return Err(ExtractionError::Wire("response tag")),
    };
    input.finish()?;
    Ok(response)
}
