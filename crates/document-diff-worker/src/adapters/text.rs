use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, SourceLocator, UnverifiedReason, WorkerChange,
    WorkerDiffRequest, WorkerDiffResponse, WorkerUnverifiedRegion,
};
use encoding_rs::UTF_8;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::WorkerError;

const MAX_TEXT_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINES: usize = 100_000;
const PARSER_PROVENANCE: &str =
    "document-diff-text-v0;encoding_rs=0.8.41;unicode-normalization=0.1.25";

#[derive(Debug, Clone, Copy, Default)]
pub struct TextComparator;

struct RawLine {
    normalized: String,
    line: u32,
    byte_start: u32,
    byte_end: u32,
}

impl RawLine {
    fn locator(&self) -> SourceLocator {
        SourceLocator::TextSpan {
            line: self.line,
            byte_start: self.byte_start,
            byte_end: self.byte_end,
        }
    }
}

impl TextComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != document_diff_core::FormatId::Txt {
            return Err(WorkerError::InvalidRequest);
        }
        request
            .validate()
            .map_err(|_| WorkerError::ResourceLimit("source bytes"))?;
        if base.len() as u64 != request.base_size_bytes
            || target.len() as u64 != request.target_size_bytes
            || <[u8; 32]>::from(Sha256::digest(base)) != request.base_raw_sha256
            || <[u8; 32]>::from(Sha256::digest(target)) != request.target_raw_sha256
        {
            return Err(WorkerError::RawBindingMismatch);
        }
        if base.len().saturating_add(target.len()) > MAX_TEXT_COMBINED_BYTES {
            return Ok(unverified(request, UnverifiedReason::ResourceLimit));
        }
        let base = match decode_lines(base) {
            Ok(lines) => lines,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let target = match decode_lines(target) {
            Ok(lines) => lines,
            Err(reason) => return Ok(unverified(request, reason)),
        };

        let mut prefix = 0;
        while prefix < base.len() && prefix < target.len() {
            if budget.consume_candidates(1).is_err() {
                return Ok(unverified(request, UnverifiedReason::ResourceLimit));
            }
            if base[prefix].normalized != target[prefix].normalized {
                break;
            }
            prefix += 1;
        }
        let mut base_end = base.len();
        let mut target_end = target.len();
        while base_end > prefix && target_end > prefix {
            if budget.consume_candidates(1).is_err() {
                return Ok(unverified(request, UnverifiedReason::ResourceLimit));
            }
            if base[base_end - 1].normalized != target[target_end - 1].normalized {
                break;
            }
            base_end -= 1;
            target_end -= 1;
        }

        let base_changed = &base[prefix..base_end];
        let target_changed = &target[prefix..target_end];
        let mut changes = Vec::new();
        if base_changed.is_empty() && target_changed.is_empty() {
            // The normalized full text is equal even when raw line endings or NFC differ.
        } else if base_changed.len() == 1 && target_changed.len() == 1 {
            changes.push(WorkerChange {
                operation: Some(ChangeOperation::Modified),
                relocation: None,
                facet: "text".to_owned(),
                base: Some(base_changed[0].locator()),
                target: Some(target_changed[0].locator()),
                reason_code: "text_changed".to_owned(),
            });
        } else if target_changed.is_empty() {
            for line in base_changed {
                changes.push(WorkerChange {
                    operation: Some(ChangeOperation::Removed),
                    relocation: None,
                    facet: "text".to_owned(),
                    base: Some(line.locator()),
                    target: None,
                    reason_code: "line_removed".to_owned(),
                });
            }
        } else if base_changed.is_empty() {
            for line in target_changed {
                changes.push(WorkerChange {
                    operation: Some(ChangeOperation::Added),
                    relocation: None,
                    facet: "text".to_owned(),
                    base: None,
                    target: Some(line.locator()),
                    reason_code: "line_added".to_owned(),
                });
            }
        } else {
            return Ok(unverified(request, UnverifiedReason::AmbiguousAlignment));
        }

        if budget.consume_changes(changes.len() as u64).is_err() {
            return Ok(unverified(request, UnverifiedReason::ResourceLimit));
        }
        Ok(response(request, DiffCoverage::Full, changes, Vec::new()))
    }
}

fn decode_lines(input: &[u8]) -> Result<Vec<RawLine>, UnverifiedReason> {
    let decoded = UTF_8
        .decode_without_bom_handling_and_without_replacement(input)
        .ok_or(UnverifiedReason::UnsupportedSemanticConstruct)?;
    let bytes = decoded.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] != b'\r' && bytes[position] != b'\n' {
            position += 1;
            continue;
        }
        let next = if bytes[position] == b'\r' && bytes.get(position + 1).copied() == Some(b'\n') {
            position + 2
        } else {
            position + 1
        };
        let mut normalized: String = decoded[start..position].nfc().collect();
        normalized.push('\n');
        push_line(&mut lines, normalized, start, next)?;
        start = next;
        position = next;
    }
    if start < bytes.len() {
        push_line(
            &mut lines,
            decoded[start..].nfc().collect(),
            start,
            bytes.len(),
        )?;
    }
    Ok(lines)
}

fn push_line(
    lines: &mut Vec<RawLine>,
    normalized: String,
    byte_start: usize,
    byte_end: usize,
) -> Result<(), UnverifiedReason> {
    if lines.len() >= MAX_LINES {
        return Err(UnverifiedReason::ResourceLimit);
    }
    let line = u32::try_from(lines.len() + 1).map_err(|_| UnverifiedReason::ResourceLimit)?;
    let byte_start = u32::try_from(byte_start).map_err(|_| UnverifiedReason::ResourceLimit)?;
    let byte_end = u32::try_from(byte_end).map_err(|_| UnverifiedReason::ResourceLimit)?;
    lines.push(RawLine {
        normalized,
        line,
        byte_start,
        byte_end,
    });
    Ok(())
}

fn unverified(request: &WorkerDiffRequest, reason: UnverifiedReason) -> WorkerDiffResponse {
    response(
        request,
        DiffCoverage::None,
        Vec::new(),
        vec![WorkerUnverifiedRegion {
            base: Some(SourceLocator::ContentItem),
            target: Some(SourceLocator::ContentItem),
            reason,
            navigation_hint: Some("原本の両側を確認してください".to_owned()),
        }],
    )
}

fn response(
    request: &WorkerDiffRequest,
    coverage: DiffCoverage,
    changes: Vec<WorkerChange>,
    unverified_regions: Vec<WorkerUnverifiedRegion>,
) -> WorkerDiffResponse {
    WorkerDiffResponse {
        protocol_version: request.protocol_version,
        diff_profile_version: request.diff_profile_version,
        resource_profile_version: request.resource_profile_version,
        base_raw_sha256: request.base_raw_sha256,
        base_size_bytes: request.base_size_bytes,
        target_raw_sha256: request.target_raw_sha256,
        target_size_bytes: request.target_size_bytes,
        format: request.format,
        coverage,
        changes,
        unverified_regions,
        parser_provenance: PARSER_PROVENANCE.to_owned(),
    }
}
