use serde::{Deserialize, Serialize};

use super::{Reader, UnitCodecError, is_nfc, write_frame, write_u32};

const PREFIX: &[u8] = b"native-locator:v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocxStep {
    BodyBlock(u32),
    Row(u32),
    Cell(u32),
    CellBlock(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PptxTextSlot {
    ShapeParagraph { paragraph: u32 },
    TableCellParagraph { row: u32, col: u32, paragraph: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeLocator {
    Docx {
        steps: Vec<DocxStep>,
    },
    Spreadsheet {
        sheet_ordinal: u32,
        row: u32,
        col: u32,
    },
    Pptx {
        slide_ordinal: u32,
        shape_path: Vec<u32>,
        text_slot: PptxTextSlot,
    },
    Pdf {
        page_index: u32,
        char_start: u32,
        char_end: u32,
    },
    Text {
        line_start: u32,
        line_end: u32,
    },
    Csv {
        record: u32,
        field: u32,
    },
    Html {
        text_node_path: Vec<u32>,
    },
    Archive {
        members: Vec<String>,
        inner: Box<NativeLocator>,
    },
}

pub fn validate_logical_path(value: &str) -> Result<(), UnitCodecError> {
    if value.is_empty()
        || !is_nfc(value)
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.len() > u32::MAX as usize
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(UnitCodecError::Invalid("logical path"));
    }
    Ok(())
}

pub fn validate_archive_member(value: &str) -> Result<(), UnitCodecError> {
    validate_logical_path(value)
}

fn validate_docx_steps(steps: &[DocxStep]) -> Result<(), UnitCodecError> {
    if !matches!(steps.first(), Some(DocxStep::BodyBlock(_))) {
        return Err(UnitCodecError::Invalid("Docx step grammar"));
    }
    let (triplets, remainder) = steps[1..].as_chunks::<3>();
    if !remainder.is_empty()
        || !triplets.iter().all(|triplet| {
            matches!(
                triplet,
                [DocxStep::Row(_), DocxStep::Cell(_), DocxStep::CellBlock(_)]
            )
        })
    {
        return Err(UnitCodecError::Invalid("Docx step grammar"));
    }
    Ok(())
}

impl NativeLocator {
    pub fn encode(&self) -> Result<Vec<u8>, UnitCodecError> {
        let mut output = PREFIX.to_vec();
        match self {
            Self::Docx { steps } => {
                validate_docx_steps(steps)?;
                output.push(1);
                write_u32(&mut output, steps.len())?;
                for step in steps {
                    let (tag, index) = match step {
                        DocxStep::BodyBlock(index) => (1, index),
                        DocxStep::Row(index) => (2, index),
                        DocxStep::Cell(index) => (3, index),
                        DocxStep::CellBlock(index) => (4, index),
                    };
                    output.push(tag);
                    output.extend_from_slice(&index.to_be_bytes());
                }
            }
            Self::Spreadsheet {
                sheet_ordinal,
                row,
                col,
            } => {
                output.push(2);
                for value in [sheet_ordinal, row, col] {
                    output.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::Pptx {
                slide_ordinal,
                shape_path,
                text_slot,
            } => {
                if shape_path.is_empty() {
                    return Err(UnitCodecError::Invalid("empty shape path"));
                }
                output.push(3);
                output.extend_from_slice(&slide_ordinal.to_be_bytes());
                write_u32(&mut output, shape_path.len())?;
                for index in shape_path {
                    output.extend_from_slice(&index.to_be_bytes());
                }
                match text_slot {
                    PptxTextSlot::ShapeParagraph { paragraph } => {
                        output.push(1);
                        output.extend_from_slice(&paragraph.to_be_bytes());
                    }
                    PptxTextSlot::TableCellParagraph {
                        row,
                        col,
                        paragraph,
                    } => {
                        output.push(2);
                        for value in [row, col, paragraph] {
                            output.extend_from_slice(&value.to_be_bytes());
                        }
                    }
                }
            }
            Self::Pdf {
                page_index,
                char_start,
                char_end,
            } => {
                if char_start >= char_end {
                    return Err(UnitCodecError::Invalid("PDF range"));
                }
                output.push(4);
                for value in [page_index, char_start, char_end] {
                    output.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::Text {
                line_start,
                line_end,
            } => {
                if line_start >= line_end {
                    return Err(UnitCodecError::Invalid("text range"));
                }
                output.push(5);
                for value in [line_start, line_end] {
                    output.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::Csv { record, field } => {
                output.push(6);
                for value in [record, field] {
                    output.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::Html { text_node_path } => {
                if text_node_path.is_empty() {
                    return Err(UnitCodecError::Invalid("empty HTML path"));
                }
                output.push(7);
                write_u32(&mut output, text_node_path.len())?;
                for index in text_node_path {
                    output.extend_from_slice(&index.to_be_bytes());
                }
            }
            Self::Archive { members, inner } => {
                if members.is_empty() || matches!(**inner, Self::Archive { .. }) {
                    return Err(UnitCodecError::Invalid("archive nesting"));
                }
                output.push(8);
                write_u32(&mut output, members.len())?;
                for member in members {
                    validate_archive_member(member)?;
                    write_frame(&mut output, member.as_bytes())?;
                }
                write_frame(&mut output, &inner.encode()?)?;
            }
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, UnitCodecError> {
        let mut input = Reader::new(bytes);
        if input.take(PREFIX.len())? != PREFIX {
            return Err(UnitCodecError::Invalid("locator prefix"));
        }
        let value = match input.byte()? {
            1 => {
                let count = input.u32()? as usize;
                if count > input.remaining() / 5 {
                    return Err(UnitCodecError::Invalid("Docx step count"));
                }
                let mut steps = Vec::new();
                for _ in 0..count {
                    let tag = input.byte()?;
                    let index = input.u32()?;
                    steps.push(match tag {
                        1 => DocxStep::BodyBlock(index),
                        2 => DocxStep::Row(index),
                        3 => DocxStep::Cell(index),
                        4 => DocxStep::CellBlock(index),
                        _ => return Err(UnitCodecError::Invalid("Docx step tag")),
                    });
                }
                validate_docx_steps(&steps)?;
                Self::Docx { steps }
            }
            2 => Self::Spreadsheet {
                sheet_ordinal: input.u32()?,
                row: input.u32()?,
                col: input.u32()?,
            },
            3 => {
                let slide_ordinal = input.u32()?;
                let count = input.u32()? as usize;
                if count == 0 || count > input.remaining() / 4 {
                    return Err(UnitCodecError::Invalid("shape path count"));
                }
                let mut shape_path = Vec::new();
                for _ in 0..count {
                    shape_path.push(input.u32()?);
                }
                let text_slot = match input.byte()? {
                    1 => PptxTextSlot::ShapeParagraph {
                        paragraph: input.u32()?,
                    },
                    2 => PptxTextSlot::TableCellParagraph {
                        row: input.u32()?,
                        col: input.u32()?,
                        paragraph: input.u32()?,
                    },
                    _ => return Err(UnitCodecError::Invalid("Pptx text slot tag")),
                };
                Self::Pptx {
                    slide_ordinal,
                    shape_path,
                    text_slot,
                }
            }
            4 => {
                let page_index = input.u32()?;
                let char_start = input.u32()?;
                let char_end = input.u32()?;
                if char_start >= char_end {
                    return Err(UnitCodecError::Invalid("PDF range"));
                }
                Self::Pdf {
                    page_index,
                    char_start,
                    char_end,
                }
            }
            5 => {
                let line_start = input.u32()?;
                let line_end = input.u32()?;
                if line_start >= line_end {
                    return Err(UnitCodecError::Invalid("text range"));
                }
                Self::Text {
                    line_start,
                    line_end,
                }
            }
            6 => Self::Csv {
                record: input.u32()?,
                field: input.u32()?,
            },
            7 => {
                let count = input.u32()? as usize;
                if count == 0 || count > input.remaining() / 4 {
                    return Err(UnitCodecError::Invalid("HTML path count"));
                }
                let mut text_node_path = Vec::new();
                for _ in 0..count {
                    text_node_path.push(input.u32()?);
                }
                Self::Html { text_node_path }
            }
            8 => {
                let count = input.u32()? as usize;
                if count == 0 || count > input.remaining() / 4 {
                    return Err(UnitCodecError::Invalid("archive member count"));
                }
                let mut members = Vec::new();
                for _ in 0..count {
                    let member = std::str::from_utf8(input.frame()?)
                        .map_err(|_| UnitCodecError::Invalid("member UTF-8"))?;
                    validate_archive_member(member)?;
                    members.push(member.to_owned());
                }
                let inner_bytes = input.frame()?;
                if inner_bytes.get(PREFIX.len()) == Some(&8) {
                    return Err(UnitCodecError::Invalid("recursive archive locator"));
                }
                let inner = Self::decode(inner_bytes)?;
                if matches!(inner, Self::Archive { .. }) {
                    return Err(UnitCodecError::Invalid("recursive archive locator"));
                }
                Self::Archive {
                    members,
                    inner: Box::new(inner),
                }
            }
            _ => return Err(UnitCodecError::Invalid("locator tag")),
        };
        input.finish()?;
        Ok(value)
    }
}
