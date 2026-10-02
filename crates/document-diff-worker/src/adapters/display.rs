use document_diff_core::{
    DisplayCell, DisplayFragment, DisplayUnavailableReason as Reason, FormatId, SourceLocator,
    UnverifiedReason, WorkerDisplayRequest,
};

use crate::WorkerError;

pub(crate) fn extract(
    request: &WorkerDisplayRequest,
    raw: &[u8],
) -> Result<DisplayFragment, WorkerError> {
    let fragment = match (request.format, &request.locator) {
        (FormatId::Txt, SourceLocator::TextSpan { .. }) => text_span(request, raw),
        (FormatId::Csv, SourceLocator::CsvCell { row, column }) => {
            csv_display_cell(raw, *row, *column)
        }
        (FormatId::Html, SourceLocator::HtmlNode { path }) => html_display_node(raw, path),
        (_, SourceLocator::ContentItem) => Ok(unavailable(Reason::Unverified)),
        (FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Pdf, _) => {
            Ok(unavailable(Reason::NonTextual))
        }
        _ => Ok(unavailable(Reason::Unsupported)),
    };
    let fragment = fragment.unwrap_or_else(unavailable);
    Ok(bound_fragment(
        fragment,
        request.max_fragment_bytes as usize,
    ))
}

fn text_span(request: &WorkerDisplayRequest, raw: &[u8]) -> Result<DisplayFragment, Reason> {
    let SourceLocator::TextSpan {
        line,
        byte_start,
        byte_end,
    } = &request.locator
    else {
        return Ok(unavailable(Reason::Unsupported));
    };
    let start = *byte_start as usize;
    let end = *byte_end as usize;
    let Some(selected) = raw.get(start..end) else {
        return Ok(unavailable(Reason::Unverified));
    };
    if actual_line(raw, start) != Some(*line) {
        return Ok(unavailable(Reason::Unverified));
    }
    let selected = selected
        .strip_suffix(b"\r\n")
        .or_else(|| selected.strip_suffix(b"\n"))
        .or_else(|| selected.strip_suffix(b"\r"))
        .unwrap_or(selected);
    let Ok(text) = std::str::from_utf8(selected) else {
        return Ok(unavailable(Reason::Unverified));
    };
    Ok(DisplayFragment::Text {
        text: text.to_owned(),
        truncated: false,
        locator: request.locator.clone(),
    })
}

fn actual_line(raw: &[u8], start: usize) -> Option<u32> {
    if start > raw.len() || (start > 0 && raw[start - 1] != b'\n' && raw[start - 1] != b'\r') {
        return None;
    }
    let mut line_breaks = 0_u32;
    for (index, byte) in raw[..start].iter().enumerate() {
        if *byte == b'\n' || (*byte == b'\r' && raw.get(index + 1) != Some(&b'\n')) {
            line_breaks = line_breaks.checked_add(1)?;
        }
    }
    line_breaks.checked_add(1)
}

fn bound_fragment(mut fragment: DisplayFragment, limit: usize) -> DisplayFragment {
    loop {
        if document_diff_core::serialized_fragment_bytes(&fragment) <= limit {
            return fragment;
        }
        let truncated = match &mut fragment {
            DisplayFragment::Text {
                text, truncated, ..
            } => {
                *truncated = true;
                trim_string(text)
            }
            DisplayFragment::Table {
                cells, truncated, ..
            } => {
                *truncated = true;
                cells
                    .last_mut()
                    .map(|cell| trim_string(&mut cell.value))
                    .unwrap_or(false)
            }
            DisplayFragment::Structural { summary, .. } => trim_string(summary),
            DisplayFragment::Unavailable { .. } => false,
        };
        if !truncated {
            return unavailable(Reason::ResourceLimit);
        }
    }
}

fn trim_string(value: &mut String) -> bool {
    if value.is_empty() {
        return false;
    }
    let boundary = value
        .char_indices()
        .nth(value.chars().count() / 2)
        .map(|(index, _)| index)
        .unwrap_or(0);
    value.truncate(boundary);
    true
}

fn unavailable(reason: Reason) -> DisplayFragment {
    DisplayFragment::Unavailable { reason }
}

fn csv_display_cell(raw: &[u8], row: u32, column: u32) -> Result<DisplayFragment, Reason> {
    let Some(value) = super::csv::display_value(raw, row, column).map_err(map_unverified)? else {
        return Ok(unavailable(Reason::Unverified));
    };
    Ok(DisplayFragment::Table {
        cells: vec![DisplayCell {
            row: Some(row),
            column: Some(column),
            label: None,
            value,
        }],
        truncated: false,
        locator: SourceLocator::CsvCell { row, column },
    })
}

fn html_display_node(raw: &[u8], path: &str) -> Result<DisplayFragment, Reason> {
    let Some(summary) = super::html::display_summary(raw, path).map_err(map_unverified)? else {
        return Ok(unavailable(Reason::Unverified));
    };
    Ok(DisplayFragment::Structural {
        summary,
        locator: SourceLocator::HtmlNode { path: path.into() },
    })
}

fn map_unverified(reason: UnverifiedReason) -> Reason {
    match reason {
        UnverifiedReason::ResourceLimit => Reason::ResourceLimit,
        _ => Reason::Unverified,
    }
}
