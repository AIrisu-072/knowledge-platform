//! Profile-bound limits with separate aggregate and per-value accounting.

use std::collections::BTreeMap;

use search_core::knowledge_unit::{BudgetKey, FormatId};
use serde::{Deserialize, Serialize};

use crate::{protocol::ReaderFailure, validation::ExtractionError};

pub const MAX_WORKER_OUTPUT_BYTES: u64 = 16_777_216;
pub const MAX_INPUT_BYTES: u64 = 268_435_456;

/// All fifteen keys are mandatory, including zero for a non-applicable key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionBudgets(BTreeMap<BudgetKey, u64>);

impl ExtractionBudgets {
    pub fn new(limits: BTreeMap<BudgetKey, u64>) -> Result<Self, ExtractionError> {
        let result = Self(limits);
        result.validate()?;
        Ok(result)
    }

    pub fn from_profile(limits: &BTreeMap<BudgetKey, u64>) -> Result<Self, ExtractionError> {
        Self::new(limits.clone())
    }

    pub fn get(&self, key: BudgetKey) -> u64 {
        self.0[&key]
    }

    pub fn limits(&self) -> &BTreeMap<BudgetKey, u64> {
        &self.0
    }

    pub fn validate(&self) -> Result<(), ExtractionError> {
        if self.0.len() != BudgetKey::ALL.len() {
            return Err(ExtractionError::Configuration("budget keys"));
        }
        for key in BudgetKey::ALL {
            let Some(&value) = self.0.get(&key) else {
                return Err(ExtractionError::Configuration("budget keys"));
            };
            if value > absolute_ceiling(key) {
                return Err(ExtractionError::Configuration("budget ceiling"));
            }
        }
        for key in [
            BudgetKey::InputBytes,
            BudgetKey::Units,
            BudgetKey::UnitUtf8Bytes,
            BudgetKey::WorkerOutputBytes,
        ] {
            if self.get(key) == 0 {
                return Err(ExtractionError::Configuration("required budget"));
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, format: FormatId) -> Result<(), ExtractionError> {
        self.validate()?;
        for key in BudgetKey::ALL {
            if !applicable(format, key) && self.get(key) != 0 {
                return Err(ExtractionError::Configuration("inapplicable budget"));
            }
        }
        Ok(())
    }
}

pub const fn absolute_ceiling(key: BudgetKey) -> u64 {
    match key {
        BudgetKey::InputBytes => MAX_INPUT_BYTES,
        BudgetKey::ZipEntries => 20_000,
        BudgetKey::ZipEntryBytes => 67_108_864,
        BudgetKey::ZipTotalBytes => 536_870_912,
        BudgetKey::ZipDepth => 3,
        BudgetKey::Units => 100_000,
        BudgetKey::UnitUtf8Bytes => 1_048_576,
        BudgetKey::WorkerOutputBytes => MAX_WORKER_OUTPUT_BYTES,
        BudgetKey::XmlDepth => 256,
        BudgetKey::XmlNodes => 2_000_000,
        BudgetKey::PdfPages => 1_024,
        BudgetKey::PdfOperations => 1_000_000,
        BudgetKey::HtmlNodes => 2_000_000,
        BudgetKey::CsvRecords => 1_000_000,
        BudgetKey::CsvFieldBytes => 1_048_576,
    }
}

const fn applicable(format: FormatId, key: BudgetKey) -> bool {
    match key {
        BudgetKey::InputBytes
        | BudgetKey::Units
        | BudgetKey::UnitUtf8Bytes
        | BudgetKey::WorkerOutputBytes => true,
        BudgetKey::ZipEntries
        | BudgetKey::ZipEntryBytes
        | BudgetKey::ZipTotalBytes
        | BudgetKey::ZipDepth => matches!(
            format,
            FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Zip
        ),
        BudgetKey::XmlDepth | BudgetKey::XmlNodes => matches!(
            format,
            FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Zip
        ),
        BudgetKey::PdfPages | BudgetKey::PdfOperations => {
            matches!(format, FormatId::Pdf | FormatId::Zip)
        }
        BudgetKey::HtmlNodes => matches!(format, FormatId::Html | FormatId::Zip),
        BudgetKey::CsvRecords | BudgetKey::CsvFieldBytes => {
            matches!(format, FormatId::Csv | FormatId::Zip)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetMeter {
    limits: ExtractionBudgets,
    charged: BTreeMap<BudgetKey, u64>,
    input_charged: bool,
}

impl BudgetMeter {
    pub fn new(limits: ExtractionBudgets) -> Result<Self, ExtractionError> {
        limits.validate()?;
        Ok(Self {
            limits,
            charged: BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect(),
            input_charged: false,
        })
    }

    pub fn charged(&self, key: BudgetKey) -> u64 {
        self.charged[&key]
    }

    /// Aggregate counters accumulate with checked arithmetic. Peak/per-value keys are
    /// compared against their individual limit, then retain the largest observed value.
    /// Failed charge leaves the meter unchanged; no overdraw can be published.
    pub fn charge(&mut self, key: BudgetKey, delta: u64) -> Result<(), ReaderFailure> {
        if per_value(key) {
            return self.observe_peak(key, delta);
        }
        if key == BudgetKey::InputBytes && self.input_charged {
            return Err(ReaderFailure::Unsupported(
                crate::coverage::CoverageReason::ResourceLimit,
            ));
        }
        let current = self.charged[&key];
        let next = current
            .checked_add(delta)
            .ok_or(ReaderFailure::Unsupported(
                crate::coverage::CoverageReason::ResourceLimit,
            ))?;
        if next > self.limits.get(key) {
            return Err(ReaderFailure::Unsupported(
                crate::coverage::CoverageReason::ResourceLimit,
            ));
        }
        self.charged.insert(key, next);
        if key == BudgetKey::InputBytes {
            self.input_charged = true;
        }
        Ok(())
    }

    /// Charge the full raw item once before parsing, including an empty raw item.
    pub fn charge_input_once(&mut self, raw_bytes: u64) -> Result<(), ReaderFailure> {
        self.charge(BudgetKey::InputBytes, raw_bytes)
    }

    pub fn check_value(&self, key: BudgetKey, value: u64) -> Result<(), ReaderFailure> {
        if !per_value(key) || value > self.limits.get(key) {
            return Err(ReaderFailure::Unsupported(
                crate::coverage::CoverageReason::ResourceLimit,
            ));
        }
        Ok(())
    }

    pub fn observe_peak(&mut self, key: BudgetKey, value: u64) -> Result<(), ReaderFailure> {
        self.check_value(key, value)?;
        self.charged.insert(key, self.charged[&key].max(value));
        Ok(())
    }
}

const fn per_value(key: BudgetKey) -> bool {
    matches!(
        key,
        BudgetKey::ZipEntryBytes
            | BudgetKey::ZipDepth
            | BudgetKey::UnitUtf8Bytes
            | BudgetKey::XmlDepth
            | BudgetKey::CsvFieldBytes
    )
}
