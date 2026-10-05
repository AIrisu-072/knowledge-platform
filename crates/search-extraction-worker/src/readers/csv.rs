//! Explicit comma/UTF-8 CSV with logical `(record, field)` locators. Quoted
//! newlines stay inside one field.

use search_core::knowledge_unit::{BudgetKey, NativeLocator, UnitKind};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::{Body, ReadResult, corrupt, resource_limit, unsupported};

pub(super) fn read(
    raw: &[u8],
    charset: &str,
    delimiter: u8,
    quote: u8,
    meter: &mut BudgetMeter,
) -> ReadResult<Body> {
    if charset != "utf-8" || delimiter != b',' || quote != b'"' {
        return Err(unsupported(CoverageReason::UnsupportedDialect));
    }
    let decoded =
        std::str::from_utf8(raw).map_err(|_| unsupported(CoverageReason::UnsupportedEncoding))?;
    // A semicolon header is an ambiguous dialect; never guess a different delimiter.
    if decoded.lines().next().unwrap_or("").contains(';') {
        return Err(unsupported(CoverageReason::UnsupportedDialect));
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .delimiter(delimiter)
        .quote(quote)
        .from_reader(raw);
    let mut body = Body::default();
    for (record_index, row) in reader.records().enumerate() {
        meter.charge(BudgetKey::CsvRecords, 1)?;
        let row = row.map_err(|_| corrupt())?;
        body.visit(1)?;
        let record = u32::try_from(record_index).map_err(|_| resource_limit())?;
        for (field_index, field) in row.iter().enumerate() {
            meter.observe_peak(BudgetKey::CsvFieldBytes, field.len() as u64)?;
            body.unit(
                meter,
                UnitKind::CsvField,
                field,
                NativeLocator::Csv {
                    record,
                    field: u32::try_from(field_index).map_err(|_| resource_limit())?,
                },
            )?;
        }
    }
    Ok(body)
}
