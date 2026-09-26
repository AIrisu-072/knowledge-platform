#![forbid(unsafe_code)]

use crate::{
    Information, Module, ModuleType, Reference, ReferenceControl, ReferenceOriginal,
    ReferenceProject, ReferenceRegistered, SysKind,
};
use codepage::to_encoding;
use encoding_rs::{CoderResult, UTF_16LE};
use nom::{
    bytes::complete::{tag, take},
    combinator::opt,
    error::{ErrorKind, ParseError},
    multi::length_data,
    number::complete::{le_u16, le_u32, le_u8},
    sequence::{preceded, tuple},
    Err::Error,
    IResult,
};

const MAX_MODULES: usize = 1_024;
const MAX_CHUNK_OUTPUT_BYTES: usize = 4_096;

// This used to be part of the public interface prior to flattening this out into the
// [`Project`] struct.
// TODO: Re-evaluate whether this struct is strictly necessary, or can be removed.
/// Specifies information for the VBA project, including project information, project
/// references, and modules.
#[derive(Debug)]
pub(crate) struct ProjectInformation {
    /// Specifies version-independent information for the VBA project.
    pub information: Information,
    /// Specifies the external references of the VBA project.
    pub references: Vec<Reference>,
    /// Specifies the modules in the project.
    pub modules: Vec<Module>,
}

// TODO: Make this error private by translating to a crate-level error type
//       at the public parser interface.
#[derive(Debug, PartialEq)]
pub(crate) enum FormatError<I> {
    UnexpectedValue,
    ResourceLimit,
    Nom(I, ErrorKind),
}

impl<I> ParseError<I> for FormatError<I> {
    fn from_error_kind(input: I, kind: ErrorKind) -> Self {
        FormatError::Nom(input, kind)
    }
    fn append(_: I, _: ErrorKind, other: Self) -> Self {
        other
    }
}

fn decode_code_page_string<I>(
    data: &[u8],
    code_page: u16,
) -> std::result::Result<String, nom::Err<FormatError<I>>> {
    cp_to_string(data, code_page).map_err(|error| match error {
        crate::Error::ResourceLimit => nom::Err::Failure(FormatError::ResourceLimit),
        _ => nom::Err::Failure(FormatError::UnexpectedValue),
    })
}

fn resource_limit_error<I>() -> nom::Err<FormatError<I>> {
    nom::Err::Failure(FormatError::ResourceLimit)
}

fn ensure_growth<I>(
    current: usize,
    additional: usize,
    limit: usize,
) -> Result<usize, nom::Err<FormatError<I>>> {
    let new_len = current
        .checked_add(additional)
        .filter(|length| *length <= limit)
        .ok_or_else(resource_limit_error::<I>)?;
    Ok(new_len)
}

fn reserve_bounded<I>(
    buffer: &mut Vec<u8>,
    additional: usize,
    limit: usize,
) -> Result<usize, nom::Err<FormatError<I>>> {
    let new_len = ensure_growth(buffer.len(), additional, limit)?;
    if new_len > buffer.capacity() {
        let target_capacity = buffer.capacity().saturating_mul(2).max(new_len).min(limit);
        buffer
            .try_reserve_exact(target_capacity - buffer.len())
            .map_err(|_| resource_limit_error())?;
    }
    Ok(new_len)
}

fn uncompressed_chunk_parser(
    i: &[u8],
    max_output: usize,
) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    if i.len() > max_output {
        return Err(resource_limit_error());
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(i.len())
        .map_err(|_| resource_limit_error())?;
    result.extend_from_slice(i);
    Ok((&[], result))
}

fn compressed_chunk_parser(
    i: &[u8],
    max_output: usize,
) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    let mut result = Vec::<u8>::new();
    // Loop until `i` is depleted
    let mut input = i;
    while !input.is_empty() {
        // Read FlagByte
        let (i, flag_byte) = le_u8(input)?;
        input = i;
        // Loop over bits
        for flag_bit_index in 0..=7 {
            // Return, if we have reached the end of this chunk
            if input.is_empty() {
                return Ok((input, result));
            }
            // Determine token type (0b0 == LiteralToken; 0b1 == CopyToken)
            let is_copy_token = (flag_byte & (1 << flag_bit_index)) != 0;
            // Delegate work based on TokenType
            if is_copy_token {
                // TODO: Move the CopyToken decoder into its own, dedicated parser.
                let (i, copy_token_raw) = le_u16(input)?;
                input = i;
                // Calculate length/offset masks
                let diff = result.len();
                let mut bit_count = 4_usize;
                while 1 << bit_count < diff {
                    bit_count += 1;
                }
                let length_mask = 0xffff_u16 >> bit_count;
                let offset_mask = !length_mask;
                // Calculate length/offset
                let length = usize::from(copy_token_raw & length_mask) + 3;
                let offset = (((copy_token_raw & offset_mask) >> (16 - bit_count)) + 1) as usize;

                if offset > result.len() {
                    return Err(nom::Err::Failure(FormatError::UnexpectedValue));
                }
                reserve_bounded(&mut result, length, max_output)?;

                // Copy bytes from the sliding window without arithmetic that can
                // underflow or index beyond the bytes already produced.
                for _ in 0..length {
                    let source_index = result.len() - offset;
                    let byte = *result
                        .get(source_index)
                        .ok_or(nom::Err::Failure(FormatError::UnexpectedValue))?;
                    result.push(byte);
                }
            } else {
                // LiteralToken -> Copy token from input stream
                let (i, byte) = le_u8(input)?;
                input = i;
                reserve_bounded(&mut result, 1, max_output)?;
                result.push(byte);
            }
        }
    }

    Ok((input, result))
}

fn chunk_parser(i: &[u8], max_output: usize) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    // CompressedChunkHeader (12 bits: size minus 3; 3 bits: 0b110; 1 bit: flag)
    // Delegate to specific parser (compressed/uncompressed) depending on the `flag`
    let (i, header_raw) = le_u16(i)?;
    // Check header magic (0b110) in bit positions 12..=14
    if (header_raw >> 12) & 0b111 != 0b011 {
        return Err(Error(FormatError::UnexpectedValue));
    }
    // Extract compressed/uncompressed flag
    let flag = ((header_raw >> 15) & 0b1) != 0;
    // Extract length
    let length = (header_raw & 0xfff) as usize + 1;

    let (remainder, chunk) = take(length)(i)?;
    if flag {
        Ok((
            remainder,
            compressed_chunk_parser(chunk, max_output.min(MAX_CHUNK_OUTPUT_BYTES))?.1,
        ))
    } else {
        Ok((remainder, uncompressed_chunk_parser(chunk, max_output)?.1))
    }
}

/// Decompress a CompressedContainer.
pub(crate) fn decompress(
    i: &[u8],
    max_output: usize,
) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const COMPRESSED_CONTAINER_SIGNATURE: &[u8] = &[0x01];
    let (mut input, _) = tag(COMPRESSED_CONTAINER_SIGNATURE)(i)?;
    let mut result = Vec::new();
    let mut parsed_chunk = false;

    while !input.is_empty() {
        let remaining_limit = max_output.saturating_sub(result.len());
        let (remainder, chunk) = chunk_parser(input, remaining_limit)?;
        reserve_bounded(&mut result, chunk.len(), max_output)?;
        result.extend_from_slice(&chunk);
        input = remainder;
        parsed_chunk = true;
    }

    if !parsed_chunk {
        return Err(nom::Err::Error(FormatError::Nom(input, ErrorKind::Many1)));
    }

    Ok((input, result))
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------

// Several size fields in the binary format have fixed values.
const U32_FIXED_SIZE_4: &[u8] = &[0x04, 0x00, 0x00, 0x00];
const U32_FIXED_SIZE_2: &[u8] = &[0x02, 0x00, 0x00, 0x00];

fn parse_syskind(i: &[u8]) -> IResult<&[u8], SysKind, FormatError<&[u8]>> {
    const SYS_KIND_SIGNATURE: &[u8] = &[0x01, 0x00];
    let (i, sys_kind) = preceded(
        tuple((tag(SYS_KIND_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        le_u32,
    )(i)?;
    match sys_kind {
        0x0000_0000 => Ok((i, SysKind::Win16)),
        0x0000_0001 => Ok((i, SysKind::Win32)),
        0x0000_0002 => Ok((i, SysKind::MacOs)),
        0x0000_0003 => Ok((i, SysKind::Win64)),
        _ => Err(Error(FormatError::UnexpectedValue)),
    }
}

fn parse_compat(i: &[u8]) -> IResult<&[u8], Option<u32>, FormatError<&[u8]>> {
    const COMPAT_SIGNATURE: &[u8] = &[0x4A, 0x00];
    let (i, compat) = opt(preceded(
        tuple((tag(COMPAT_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        le_u32,
    ))(i)?;
    Ok((i, compat))
}

fn parse_lcid(i: &[u8]) -> IResult<&[u8], u32, FormatError<&[u8]>> {
    const LCID_SIGNATURE: &[u8] = &[0x02, 0x00];
    let (i, lcid) = preceded(tuple((tag(LCID_SIGNATURE), tag(U32_FIXED_SIZE_4))), le_u32)(i)?;
    Ok((i, lcid))
}

fn parse_lcid_invoke(i: &[u8]) -> IResult<&[u8], u32, FormatError<&[u8]>> {
    const LCID_INVOKE_SIGNATURE: &[u8] = &[0x14, 0x00];
    let (i, lcid_invoke) = preceded(
        tuple((tag(LCID_INVOKE_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        le_u32,
    )(i)?;
    Ok((i, lcid_invoke))
}

fn parse_code_page(i: &[u8]) -> IResult<&[u8], u16, FormatError<&[u8]>> {
    const CODE_PAGE_SIGNATURE: &[u8] = &[0x03, 0x00];
    let (i, code_page) = preceded(
        tuple((tag(CODE_PAGE_SIGNATURE), tag(U32_FIXED_SIZE_2))),
        le_u16,
    )(i)?;
    Ok((i, code_page))
}

fn parse_name(i: &[u8]) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const NAME_SIGNATURE: &[u8] = &[0x04, 0x00];
    let (i, name) = preceded(tag(NAME_SIGNATURE), length_data(le_u32))(i)?;
    Ok((i, name.to_vec()))
}

fn parse_doc_string(i: &[u8]) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const DOC_STRING_SIGNATURE: &[u8] = &[0x05, 0x00];
    let (i, doc_string) = preceded(tag(DOC_STRING_SIGNATURE), length_data(le_u32))(i)?;
    Ok((i, doc_string.to_vec()))
}

fn parse_doc_string_unicode(i: &[u8]) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const DOC_STRING_UNICODE_SIGNATURE: &[u8] = &[0x40, 0x00];
    let (i, doc_string_unicode) =
        preceded(tag(DOC_STRING_UNICODE_SIGNATURE), length_data(le_u32))(i)?;
    // `doc_string_unicode` represents a sequence of UTF-16 code units. If its length is uneven,
    // the input is malformed.
    if (doc_string_unicode.len() & 1_usize) != 0 {
        Err(Error(FormatError::UnexpectedValue))
    } else {
        Ok((i, doc_string_unicode.to_vec()))
    }
}

fn parse_help_file_1(i: &[u8]) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const HELP_FILE_1_SIGNATURE: &[u8] = &[0x06, 0x00];
    let (i, help_file_1) = preceded(tag(HELP_FILE_1_SIGNATURE), length_data(le_u32))(i)?;
    Ok((i, help_file_1.to_vec()))
}

fn parse_help_file_2(i: &[u8]) -> IResult<&[u8], Vec<u8>, FormatError<&[u8]>> {
    const HELP_FILE_2_SIGNATURE: &[u8] = &[0x3d, 0x00];
    let (i, help_file_2) = preceded(tag(HELP_FILE_2_SIGNATURE), length_data(le_u32))(i)?;
    Ok((i, help_file_2.to_vec()))
}

fn parse_help_context(i: &[u8]) -> IResult<&[u8], u32, FormatError<&[u8]>> {
    const HELP_CONTEXT_SIGNATURE: &[u8] = &[0x07, 0x00];
    let (i, help_context) = preceded(
        tuple((tag(HELP_CONTEXT_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        le_u32,
    )(i)?;
    Ok((i, help_context))
}

fn parse_lib_flags(i: &[u8]) -> IResult<&[u8], u32, FormatError<&[u8]>> {
    const LIB_FLAGS_SIGNATURE: &[u8] = &[0x08, 0x00];
    let (i, lib_flags) = preceded(
        tuple((tag(LIB_FLAGS_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        le_u32,
    )(i)?;
    Ok((i, lib_flags))
}

fn parse_version(i: &[u8]) -> IResult<&[u8], (u32, u16), FormatError<&[u8]>> {
    const VERSION_SIGNATURE: &[u8] = &[0x09, 0x00];
    let (i, version) = preceded(
        tuple((tag(VERSION_SIGNATURE), tag(U32_FIXED_SIZE_4))),
        tuple((le_u32, le_u16)),
    )(i)?;
    Ok((i, version))
}

fn parse_constants(i: &[u8]) -> IResult<&[u8], Option<Vec<u8>>, FormatError<&[u8]>> {
    const CONSTANTS_SIGNATURE: &[u8] = &[0x0c, 0x00];
    let (i, constants) = opt(preceded(tag(CONSTANTS_SIGNATURE), length_data(le_u32)))(i)?;
    let constants = constants.map(|slice| slice.to_vec());
    Ok((i, constants))
}

fn parse_constants_unicode(i: &[u8]) -> IResult<&[u8], Option<Vec<u8>>, FormatError<&[u8]>> {
    const CONSTANTS_UNICODE_SIGNATURE: &[u8] = &[0x3c, 0x00];
    let (i, constants_unicode) = opt(preceded(
        tag(CONSTANTS_UNICODE_SIGNATURE),
        length_data(le_u32),
    ))(i)?;
    let constants_unicode = constants_unicode.map(|slice| slice.to_vec());
    Ok((i, constants_unicode))
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn parse_reference_name(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], Option<String>, FormatError<&[u8]>> {
    const NAME_SIGNATURE: &[u8] = &[0x16, 0x00];
    const NAME_UNICODE_SIGNATURE: &[u8] = &[0x3e, 0x00];
    let (i, name) = opt(tuple((
        preceded(tag(NAME_SIGNATURE), length_data(le_u32)),
        preceded(tag(NAME_UNICODE_SIGNATURE), length_data(le_u32)),
    )))(i)?;
    // name_unicode MUST contain the UTF-16 encoding of name. Can be dropped without
    // loss of information.
    if let Some((name, _name_unicode)) = name {
        let name = decode_code_page_string(name, code_page)?;
        Ok((i, Some(name)))
    } else {
        Ok((i, None))
    }
}

fn parse_reference_original(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], String, FormatError<&[u8]>> {
    const ORIGINAL_SIGNATURE: &[u8] = &[0x33, 0x00];
    let (i, libid_original) = preceded(tag(ORIGINAL_SIGNATURE), length_data(le_u32))(i)?;
    let libid_original = decode_code_page_string(libid_original, code_page)?;
    Ok((i, libid_original))
}

fn parse_reference_control(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], ReferenceControl, FormatError<&[u8]>> {
    // REFERENCEORIGINAL Record is optional here
    let (_, id) = le_u16(i)?;
    let (i, libid_original) = match id {
        0x0033_u16 => {
            let (i, libid_original) = parse_reference_original(i, code_page)?;
            (i, Some(libid_original))
        }
        _ => (i, None),
    };

    const CONTROL_SIGNATURE: &[u8] = &[0x2f, 0x00];
    let (i, libid_twiddled) =
        preceded(tuple((tag(CONTROL_SIGNATURE), le_u32)), length_data(le_u32))(i)?;
    let libid_twiddled = decode_code_page_string(libid_twiddled, code_page)?;

    const RESERVED_1: &[u8] = &[0x00, 0x00, 0x00, 0x00];
    const RESERVED_2: &[u8] = &[0x00, 0x00];
    let (i, _) = tuple((tag(RESERVED_1), tag(RESERVED_2)))(i)?;

    let (i, name_extended) = parse_reference_name(i, code_page)?;

    const RESERVED_3: &[u8] = &[0x30, 0x00];
    let (i, libid_extended) = preceded(tuple((tag(RESERVED_3), le_u32)), length_data(le_u32))(i)?;
    let libid_extended = decode_code_page_string(libid_extended, code_page)?;

    const RESERVED_4: &[u8] = &[0x00, 0x00, 0x00, 0x00];
    const RESERVED_5: &[u8] = &[0x00, 0x00];
    let (i, _) = tuple((tag(RESERVED_4), tag(RESERVED_5)))(i)?;

    let (i, guid) = take(16_usize)(i)?;
    let guid = guid.to_vec();

    let (i, cookie) = le_u32(i)?;

    Ok((
        i,
        ReferenceControl {
            name: None,
            libid_original,
            libid_twiddled,
            name_extended,
            libid_extended,
            guid,
            cookie,
        },
    ))
}

fn parse_reference_registered(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], ReferenceRegistered, FormatError<&[u8]>> {
    const REGISTERED_SIGNATURE: &[u8] = &[0x0d, 0x00];
    let (i, libid) = preceded(
        tuple((tag(REGISTERED_SIGNATURE), le_u32)),
        length_data(le_u32),
    )(i)?;
    let libid = decode_code_page_string(libid, code_page)?;

    const RESERVED_1: &[u8] = &[0x00, 0x00, 0x00, 0x00];
    const RESERVED_2: &[u8] = &[0x00, 0x00];
    let (i, _) = tuple((tag(RESERVED_1), tag(RESERVED_2)))(i)?;

    Ok((i, ReferenceRegistered { name: None, libid }))
}

fn parse_reference_project(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], ReferenceProject, FormatError<&[u8]>> {
    let (i, (libid_absolute, libid_relative, major_version, minor_version)) = tuple((
        preceded(tuple((tag(&[0x0e, 0x00]), le_u32)), length_data(le_u32)),
        length_data(le_u32),
        le_u32,
        le_u16,
    ))(i)?;
    let libid_absolute = decode_code_page_string(libid_absolute, code_page)?;
    let libid_relative = decode_code_page_string(libid_relative, code_page)?;

    Ok((
        i,
        ReferenceProject {
            name: None,
            libid_absolute,
            libid_relative,
            major_version,
            minor_version,
        },
    ))
}

/// Parses a single REFERENCE Record.
///
/// There are several tricky bits to this:
/// * The first entry (NameRecord) is optional.
/// * The REFERENCE Record can be one of 4 variants.
/// * The length is implied through a terminator (0x000F) that starts a PROJECTMODULES Record.
///
/// Returns `Some(reference)` if a variant was found, `None` if the end of the array was
/// reached, or an error.
fn parse_reference(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], Option<Reference>, FormatError<&[u8]>> {
    let (i, name) = parse_reference_name(i, code_page)?;
    // Determine REFERENCE Record variant (or end of array)
    let (_, id) = le_u16(i)?;
    match id {
        0x002f_u16 => {
            let (i, mut value) = parse_reference_control(i, code_page)?;
            value.name = name;
            Ok((i, Some(Reference::Control(value))))
        }
        0x0033_u16 => {
            let (i, libid_original) = parse_reference_original(i, code_page)?;
            let original = ReferenceOriginal {
                name,
                libid_original,
            };
            Ok((i, Some(Reference::Original(original))))
        }
        0x000d_u16 => {
            let (i, mut value) = parse_reference_registered(i, code_page)?;
            value.name = name;
            Ok((i, Some(Reference::Registered(value))))
        }
        0x000e_u16 => {
            let (i, mut value) = parse_reference_project(i, code_page)?;
            value.name = name;
            Ok((i, Some(Reference::Project(value))))
        }
        0x000f_u16 => Ok((i, None)),
        _ => Err(Error(FormatError::UnexpectedValue)),
    }
}

fn parse_references(
    i: &[u8],
    code_page: u16,
) -> IResult<&[u8], Vec<Reference>, FormatError<&[u8]>> {
    let mut result = Vec::new();
    let mut i = i;
    loop {
        let (remainder, value) = parse_reference(i, code_page)?;
        i = remainder;
        if let Some(reference) = value {
            result.push(reference);
        } else {
            return Ok((i, result));
        }
    }
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------

fn parse_module(i: &[u8], code_page: u16) -> IResult<&[u8], Module, FormatError<&[u8]>> {
    // MODULENAME Record
    let (i, name) = preceded(tag(&[0x19, 0x00]), length_data(le_u32))(i)?;
    let name = decode_code_page_string(name, code_page)?;

    // (Optional) MODULENAMEUNICODE Record
    // If present it MUST be the UTF-16 encoding of MODULENAME. It can safely be dropped.
    let (i, _name_unicode) = opt(preceded(tag(&[0x47, 0x00]), length_data(le_u32)))(i)?;

    // MODULESTREAMNAME Record
    // stream_name_unicode MUST be the UTF-16 encoding of stream_name. It can safely be dropped.
    let (i, (stream_name, _stream_name_unicode)) = tuple((
        preceded(tag(&[0x1a, 0x00]), length_data(le_u32)),
        preceded(tag(&[0x32, 0x00]), length_data(le_u32)),
    ))(i)?;
    let stream_name = decode_code_page_string(stream_name, code_page)?;

    // MODULEDOCSTRING Record
    // doc_string_unicode MUST be the UTF-16 encoding of doc_string. It can safely be dropped.
    let (i, (doc_string, _doc_string_unicode)) = tuple((
        preceded(tag(&[0x1c, 0x00]), length_data(le_u32)),
        preceded(tag(&[0x48, 0x00]), length_data(le_u32)),
    ))(i)?;
    let doc_string = decode_code_page_string(doc_string, code_page)?;

    // MODULEOFFSET Record
    let (i, text_offset) = preceded(tuple((tag(&[0x31, 0x00]), tag(U32_FIXED_SIZE_4))), le_u32)(i)?;
    let text_offset = text_offset as _;

    // MODULEHELPCONTEXT Record
    let (i, help_context) =
        preceded(tuple((tag(&[0x1e, 0x00]), tag(U32_FIXED_SIZE_4))), le_u32)(i)?;

    // MODULECOOKIE Record
    // Cookie MUST be ignored on read.
    let (i, _cookie) = preceded(tuple((tag(&[0x2c, 0x00]), tag(U32_FIXED_SIZE_2))), le_u16)(i)?;

    // MODULETYPE Record
    let (i, id) = le_u16(i)?;
    let module_type = match id {
        0x0021_u16 => ModuleType::Procedural,
        0x0022_u16 => ModuleType::DocClsDesigner,
        _ => return Err(Error(FormatError::UnexpectedValue)),
    };
    let (i, _) = tag(&[0x00, 0x00, 0x00, 0x00])(i)?;

    // MODULEREADONLY Record
    let (i, read_only) = opt(tag(&[0x25, 0x00, 0x00, 0x00, 0x00, 0x00]))(i)?;
    let read_only = read_only.is_some();

    // MODULEPRIVATE Record
    let (i, private) = opt(tag(&[0x28, 0x00, 0x00, 0x00, 0x00, 0x00]))(i)?;
    let private = private.is_some();

    // Terminator
    let (i, _) = tag(&[0x2b, 0x00])(i)?;

    // Reserved
    let (i, _) = tag(&[0x00, 0x00, 0x00, 0x00])(i)?;

    Ok((
        i,
        Module {
            name,
            stream_name,
            doc_string,
            text_offset,
            help_context,
            module_type,
            read_only,
            private,
        },
    ))
}

fn parse_modules(i: &[u8], code_page: u16) -> IResult<&[u8], Vec<Module>, FormatError<&[u8]>> {
    let (i, count) = preceded(tuple((tag(&[0x0f, 0x00]), tag(U32_FIXED_SIZE_2))), le_u16)(i)?;
    let count = usize::from(count);
    if count > MAX_MODULES {
        return Err(nom::Err::Failure(FormatError::ResourceLimit));
    }
    // Cookie MUST be ignored on read.
    let (i, _cookie) = preceded(tuple((tag(&[0x13, 0x00]), tag(U32_FIXED_SIZE_2))), le_u16)(i)?;

    let mut modules = Vec::with_capacity(count);
    let mut i = i;
    for _ in 0..count {
        let (remainder, module) = parse_module(i, code_page)?;
        i = remainder;
        modules.push(module);
    }

    Ok((i, modules))
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------

/// *dir* stream parser.
pub(crate) fn parse_project_information(
    i: &[u8],
) -> IResult<&[u8], ProjectInformation, FormatError<&[u8]>> {
    let (i, sys_kind) = parse_syskind(i)?;
    let (i, compat) = parse_compat(i)?;
    let (i, lcid) = parse_lcid(i)?;
    let (i, lcid_invoke) = parse_lcid_invoke(i)?;
    let (i, code_page) = parse_code_page(i)?;

    let (i, name) = parse_name(i)?;
    let name = decode_code_page_string(&name, code_page)?;

    let (i, doc_string) = parse_doc_string(i)?;
    let doc_string = decode_code_page_string(&doc_string, code_page)?;

    // doc_string_unicode MUST contain the UTF-16 encoding of doc_string. Can safely be dropped.
    let (i, _doc_string_unicode) = parse_doc_string_unicode(i)?;

    let (i, help_file_1) = parse_help_file_1(i)?;
    let help_file_1 = decode_code_page_string(&help_file_1, code_page)?;

    // help_file_2 MUST contain the same bytes as help_file_1. Can safely be dropped.
    let (i, _help_file_2) = parse_help_file_2(i)?;

    let (i, help_context) = parse_help_context(i)?;
    let (i, lib_flags) = parse_lib_flags(i)?;
    let (i, (version_major, version_minor)) = parse_version(i)?;

    // The `PROJECTCONSTANTS` record is optional (as a whole); make sure to only parse the
    // Unicode portion if `parse_constants` returned `Some`.
    //
    // TODO: Consider consolidating CP and Unicode parsing into a single function. This
    // would avoid having to subsequently deal with the outcome of this function.
    let (i, constants) = parse_constants(i)?;
    let constants = constants
        .map(|constants| decode_code_page_string(&constants, code_page))
        .transpose()?;

    let i = if constants.is_some() {
        // constants_unicode MUST contain the UTF-16 encoding of constants. Can safely be
        // dropped.
        let (i, _constants_unicode) = parse_constants_unicode(i)?;
        i
    } else {
        i
    };

    let (i, references) = parse_references(i, code_page)?;

    let (i, modules) = parse_modules(i, code_page)?;

    // Terminator
    let (i, _) = tag(&[0x10, 0x00])(i)?;

    // Reserved
    let (i, _) = tag(&[0x00, 0x00, 0x00, 0x00])(i)?;

    if !i.is_empty() {
        return Err(nom::Err::Failure(FormatError::UnexpectedValue));
    }

    Ok((
        i,
        ProjectInformation {
            information: Information {
                sys_kind,
                compat,
                lcid,
                lcid_invoke,
                code_page,
                name,
                doc_string,
                help_file_1,
                help_context,
                lib_flags,
                version_major,
                version_minor,
                constants,
            },
            references,
            modules,
        },
    ))
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------

pub(crate) fn cp_to_string(data: &[u8], code_page: u16) -> crate::Result<String> {
    let encoding = to_encoding(code_page).ok_or(crate::Error::Parser)?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let max_length = decoder
        .max_utf8_buffer_length(data.len())
        .ok_or(crate::Error::ResourceLimit)?;
    let mut result = String::new();
    result
        .try_reserve_exact(max_length)
        .map_err(|_| crate::Error::ResourceLimit)?;
    let (decoder_result, bytes_read, had_errors) =
        decoder.decode_to_string(data, &mut result, true);
    if decoder_result != CoderResult::InputEmpty || bytes_read != data.len() || had_errors {
        return Err(crate::Error::Parser);
    }

    Ok(result)
}

#[allow(dead_code)]
fn utf16_to_string(data: &[u8]) -> String {
    let mut decoder = UTF_16LE.new_decoder_without_bom_handling();
    let max_length = decoder.max_utf8_buffer_length(data.len()).unwrap();
    let mut result = String::with_capacity(max_length);
    let (decoder_result, _, _) = decoder.decode_to_string(data, &mut result, true);
    assert_eq!(
        decoder_result,
        CoderResult::InputEmpty,
        "Failed to decode full UTF-16 sequence."
    );

    result
}
