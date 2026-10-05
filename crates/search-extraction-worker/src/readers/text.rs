//! Strictly decoded plain text with physical line half-open ranges.

use encoding_rs::SHIFT_JIS;
use search_core::knowledge_unit::{NativeLocator, UnitKind, normalize_unit_text};
use search_extraction_core::{BudgetMeter, CoverageReason};

use super::{Body, ReadResult, resource_limit, unsupported};

const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];

pub(super) fn read(raw: &[u8], charset: &str, meter: &mut BudgetMeter) -> ReadResult<Body> {
    let decoded = match charset {
        "utf-8" => std::str::from_utf8(raw.strip_prefix(UTF8_BOM).unwrap_or(raw))
            .map_err(|_| unsupported(CoverageReason::UnsupportedEncoding))?
            .to_owned(),
        // WHATWG Shift_JIS is the Windows-31J mapping; invalid bytes never become U+FFFD.
        "windows-31j" => SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(raw)
            .ok_or(unsupported(CoverageReason::UnsupportedEncoding))?
            .into_owned(),
        _ => return Err(unsupported(CoverageReason::UnsupportedEncoding)),
    };
    let mut body = Body::default();
    for (index, line) in normalize_unit_text(&decoded).split('\n').enumerate() {
        body.visit(1)?;
        let line_start = u32::try_from(index).map_err(|_| resource_limit())?;
        let line_end = line_start.checked_add(1).ok_or_else(resource_limit)?;
        body.unit(
            meter,
            UnitKind::PlainText,
            line,
            NativeLocator::Text {
                line_start,
                line_end,
            },
        )?;
    }
    Ok(body)
}
