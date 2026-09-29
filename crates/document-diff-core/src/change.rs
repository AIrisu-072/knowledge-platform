use serde::{Deserialize, Serialize};

use crate::DiffCoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentVerdict {
    Same,
    Different,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffCoverage {
    Full,
    Partial,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOperation {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelocationKind {
    Moved,
    Reordered,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnverifiedReason {
    UnsupportedSemanticConstruct,
    CorruptedSource,
    MissingInspectionEvidence,
    AmbiguousAlignment,
    ResourceLimit,
}

impl UnverifiedReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedSemanticConstruct => "unsupported_semantic_construct",
            Self::CorruptedSource => "corrupted_source",
            Self::MissingInspectionEvidence => "missing_inspection_evidence",
            Self::AmbiguousAlignment => "ambiguous_alignment",
            Self::ResourceLimit => "resource_limit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceLocator {
    ContentItem,
    TextSpan {
        line: u32,
        byte_start: u32,
        byte_end: u32,
    },
    CsvCell {
        row: u32,
        column: u32,
    },
    HtmlNode {
        path: String,
    },
    OfficePath {
        path: String,
    },
    SheetCell {
        sheet: String,
        cell: String,
    },
    VbaModule {
        module: String,
        procedure: Option<String>,
    },
    SlideObject {
        slide: u32,
        object: Option<String>,
    },
    PdfPage {
        page: u32,
        region: Option<[u32; 4]>,
    },
}

impl SourceLocator {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        match self {
            Self::ContentItem => Ok(()),
            Self::TextSpan {
                line,
                byte_start,
                byte_end,
            } if *line > 0 && byte_end > byte_start => Ok(()),
            Self::CsvCell { row, column } if *row > 0 && *column > 0 => Ok(()),
            Self::HtmlNode { path } | Self::OfficePath { path } if valid_label(path) => Ok(()),
            Self::SheetCell { sheet, cell } if valid_label(sheet) && valid_label(cell) => Ok(()),
            Self::VbaModule { module, procedure }
                if valid_label(module) && procedure.as_ref().is_none_or(|p| valid_label(p)) =>
            {
                Ok(())
            }
            Self::SlideObject { slide, object }
                if *slide > 0 && object.as_ref().is_none_or(|o| valid_label(o)) =>
            {
                Ok(())
            }
            Self::PdfPage { page, region }
                if *page > 0 && region.as_ref().is_none_or(|r| r[2] > r[0] && r[3] > r[1]) =>
            {
                Ok(())
            }
            _ => Err(DiffCoreError::InvalidResponse(
                "invalid source locator".to_owned(),
            )),
        }
    }
}

fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerChange {
    pub operation: Option<ChangeOperation>,
    pub relocation: Option<RelocationKind>,
    pub facet: String,
    pub base: Option<SourceLocator>,
    pub target: Option<SourceLocator>,
    pub reason_code: String,
}

impl WorkerChange {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if self.operation.is_none() && self.relocation.is_none() {
            return Err(DiffCoreError::InvalidResponse(
                "change has no operation or relocation".to_owned(),
            ));
        }
        if !valid_label(&self.facet) || !valid_label(&self.reason_code) {
            return Err(DiffCoreError::InvalidResponse(
                "change facet or reason is blank".to_owned(),
            ));
        }
        let sides_valid = match self.operation {
            Some(ChangeOperation::Added) => self.base.is_none() && self.target.is_some(),
            Some(ChangeOperation::Removed) => self.base.is_some() && self.target.is_none(),
            Some(ChangeOperation::Modified) | None => self.base.is_some() && self.target.is_some(),
        };
        if !sides_valid {
            return Err(DiffCoreError::InvalidResponse(
                "change has invalid source sides".to_owned(),
            ));
        }
        if let Some(locator) = &self.base {
            locator.validate()?;
        }
        if let Some(locator) = &self.target {
            locator.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerUnverifiedRegion {
    pub base: Option<SourceLocator>,
    pub target: Option<SourceLocator>,
    pub reason: UnverifiedReason,
    pub navigation_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerAncillaryChange {
    pub kind: String,
    pub base_digest: Option<[u8; 32]>,
    pub target_digest: Option<[u8; 32]>,
}

impl WorkerAncillaryChange {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if !valid_label(&self.kind) || (self.base_digest.is_none() && self.target_digest.is_none())
        {
            return Err(DiffCoreError::InvalidResponse(
                "invalid ancillary change".to_owned(),
            ));
        }
        Ok(())
    }
}

impl WorkerUnverifiedRegion {
    pub fn validate(&self) -> Result<(), DiffCoreError> {
        if self.base.is_none() && self.target.is_none() {
            return Err(DiffCoreError::InvalidResponse(
                "unverified region has no source".to_owned(),
            ));
        }
        if let Some(locator) = &self.base {
            locator.validate()?;
        }
        if let Some(locator) = &self.target {
            locator.validate()?;
        }
        if self
            .navigation_hint
            .as_ref()
            .is_some_and(|h| !valid_label(h))
        {
            return Err(DiffCoreError::InvalidResponse(
                "invalid navigation hint".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComparisonBudget {
    max_candidates: u64,
    max_changes: u64,
    candidates: u64,
    changes: u64,
}

impl ComparisonBudget {
    pub const fn new(max_candidates: u64, max_changes: u64) -> Self {
        Self {
            max_candidates,
            max_changes,
            candidates: 0,
            changes: 0,
        }
    }

    pub fn consume_candidates(&mut self, count: u64) -> Result<(), DiffCoreError> {
        self.candidates = charge(self.candidates, count, self.max_candidates, "candidates")?;
        Ok(())
    }

    pub fn consume_changes(&mut self, count: u64) -> Result<(), DiffCoreError> {
        self.changes = charge(self.changes, count, self.max_changes, "changes")?;
        Ok(())
    }

    pub const fn candidates_used(&self) -> u64 {
        self.candidates
    }
    pub const fn changes_used(&self) -> u64 {
        self.changes
    }
}

fn charge(current: u64, count: u64, limit: u64, class: &'static str) -> Result<u64, DiffCoreError> {
    let next = current
        .checked_add(count)
        .ok_or(DiffCoreError::ResourceLimit(class))?;
    if next > limit {
        return Err(DiffCoreError::ResourceLimit(class));
    }
    Ok(next)
}
