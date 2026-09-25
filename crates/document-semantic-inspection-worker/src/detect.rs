use document_semantic_inspection_core::FormatId;

use crate::{WorkerFailure, WorkerFailureCode};

const DOCX_MEDIA: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX_MEDIA: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const XLSM_MEDIA: &str = "application/vnd.ms-excel.sheet.macroenabled.12";
const PPTX_MEDIA: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

pub fn detect_format(
    bytes: &[u8],
    declared_media_type: &str,
) -> Result<FormatId, WorkerFailure> {
    let declared = normalize_media_type(declared_media_type);
    let expected = declared_format(&declared);

    let detected = if bytes.starts_with(b"%PDF-") {
        Some(FormatId::Pdf)
    } else if bytes.starts_with(b"PK\x03\x04")
        || bytes.windows(4).any(|window| window == b"PK\x01\x02")
    {
        detect_ooxml(bytes)
    } else if is_textual(bytes) && looks_like_html(bytes) {
        Some(FormatId::Html)
    } else if is_textual(bytes) && expected == Some(FormatId::Csv) && looks_like_csv(bytes) {
        Some(FormatId::Csv)
    } else if is_textual(bytes) && expected == Some(FormatId::Txt) {
        Some(FormatId::Txt)
    } else {
        None
    };

    let Some(detected) = detected else {
        return Err(WorkerFailure::new(
            WorkerFailureCode::UnsupportedDocumentFormat,
            "content does not match a supported Document Semantic Inspection v0 format",
        ));
    };

    let Some(expected) = expected else {
        return Err(WorkerFailure::new(
            WorkerFailureCode::UnsupportedDocumentFormat,
            "declared media type is not supported by Document Semantic Inspection v0",
        ));
    };

    if detected != expected {
        return Err(WorkerFailure::new(
            WorkerFailureCode::FormatMismatch,
            format!(
                "detected format {detected:?} is incompatible with declared media type {declared_media_type}"
            ),
        ));
    }

    Ok(detected)
}

fn normalize_media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn declared_format(media_type: &str) -> Option<FormatId> {
    match media_type {
        DOCX_MEDIA => Some(FormatId::Docx),
        XLSX_MEDIA => Some(FormatId::Xlsx),
        XLSM_MEDIA => Some(FormatId::Xlsm),
        PPTX_MEDIA => Some(FormatId::Pptx),
        "application/pdf" => Some(FormatId::Pdf),
        "text/plain" => Some(FormatId::Txt),
        "text/csv" | "application/csv" => Some(FormatId::Csv),
        "text/html" | "application/xhtml+xml" => Some(FormatId::Html),
        _ => None,
    }
}

fn detect_ooxml(bytes: &[u8]) -> Option<FormatId> {
    let names = central_directory_names(bytes);
    if !names.iter().any(|name| name == "[Content_Types].xml") {
        return None;
    }

    if names.iter().any(|name| name == "word/document.xml") {
        return Some(FormatId::Docx);
    }
    if names.iter().any(|name| name == "ppt/presentation.xml") {
        return Some(FormatId::Pptx);
    }
    if names.iter().any(|name| name == "xl/workbook.xml") {
        if names.iter().any(|name| name == "xl/vbaProject.bin") {
            return Some(FormatId::Xlsm);
        }
        return Some(FormatId::Xlsx);
    }

    None
}

fn central_directory_names(bytes: &[u8]) -> Vec<String> {
    const SIGNATURE: &[u8; 4] = b"PK\x01\x02";
    const HEADER_LEN: usize = 46;

    let mut names = Vec::new();
    let mut cursor = 0;

    while cursor + HEADER_LEN <= bytes.len() {
        let Some(relative) = bytes[cursor..]
            .windows(SIGNATURE.len())
            .position(|window| window == SIGNATURE)
        else {
            break;
        };
        let start = cursor + relative;
        if start + HEADER_LEN > bytes.len() {
            break;
        }

        let file_name_len = u16::from_le_bytes([bytes[start + 28], bytes[start + 29]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[start + 30], bytes[start + 31]]) as usize;
        let comment_len = u16::from_le_bytes([bytes[start + 32], bytes[start + 33]]) as usize;

        let name_start = start + HEADER_LEN;
        let Some(name_end) = name_start.checked_add(file_name_len) else {
            break;
        };
        let Some(next) = name_end
            .checked_add(extra_len)
            .and_then(|value| value.checked_add(comment_len))
        else {
            break;
        };
        if next > bytes.len() {
            break;
        }

        if let Ok(name) = std::str::from_utf8(&bytes[name_start..name_end]) {
            names.push(name.replace('\\', "/"));
        }
        cursor = next;
    }

    names
}

fn is_textual(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'\t' | b'\n' | b'\r') || *byte >= 0x20)
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let lower: Vec<u8> = bytes.iter().map(u8::to_ascii_lowercase).collect();
    [b"<!doctype html".as_slice(), b"<html".as_slice(), b"<body".as_slice()]
        .iter()
        .any(|needle| lower.windows(needle.len()).any(|window| window == *needle))
}

fn looks_like_csv(bytes: &[u8]) -> bool {
    let Some(line_end) = bytes.iter().position(|byte| *byte == b'\n' || *byte == b'\r') else {
        return false;
    };
    bytes[..line_end]
        .iter()
        .any(|byte| matches!(byte, b',' | b';' | b'\t'))
}
