use super::parser::{decompress, parse_project_information};

#[test]
fn copy_token_decoder() {
    // CopyTokens store offset and length information in a single 16-bit value. The bit
    // range used for either one changes with the current position in the output stream.
    //
    // This test verifies the implementation by running the decompressor against input
    // where the first CopyToken is encountered at positions 31, 32, and 33 in the
    // output stream.
    //
    // The input was generated using Excel 2013, by adding non-repeating character
    // sequences to a module, until the full size reached the desired length.
    // The prefix `Attribute VB_Name = "a"\r\n` gets added by Excel for every code
    // module, where `"a"` is the respective module name.
    // The resulting *vbaProject.bin* files were then extracted from the Excel documents,
    // opened in a hex editor, and the byte sequences corresponding to the respective
    // compressed containers copied here.

    // CompressedContainer with first CopyToken at position 31:
    // 01 27 B0 00 41 74 74 72 69 62 75 74 00 65 20 56 42 5F 4E 61 6D 00 65 20 3D 20 22 61 22 0D 80 0A 61 62 63 64 65 66 06 F0 00 0D 0A
    const CONTAINER_1: &[u8] = b"\x01\x27\xB0\x00\x41\x74\x74\x72\x69\x62\x75\x74\x00\x65\x20\x56\x42\x5F\x4E\x61\x6D\x00\x65\x20\x3D\x20\x22\x61\x22\x0D\x80\x0A\x61\x62\x63\x64\x65\x66\x06\xF0\x00\x0D\x0A";
    const CONTENTS_1: &[u8] = b"Attribute VB_Name = \"a\"\x0D\x0AabcdefAttribute\x0D\x0A";
    let contents = decompress(CONTAINER_1, 4096).unwrap().1;
    assert_eq!(contents, CONTENTS_1);

    // CompressedContainer with first CopyToken at position 32:
    // 01 28 B0 00 41 74 74 72 69 62 75 74 00 65 20 56 42 5F 4E 61 6D 00 65 20 3D 20 22 61 22 0D 00 0A 61 62 63 64 65 66 67 01 06 F8 0D 0A
    const CONTAINER_2: &[u8] = b"\x01\x28\xB0\x00\x41\x74\x74\x72\x69\x62\x75\x74\x00\x65\x20\x56\x42\x5F\x4E\x61\x6D\x00\x65\x20\x3D\x20\x22\x61\x22\x0D\x00\x0A\x61\x62\x63\x64\x65\x66\x67\x01\x06\xF8\x0D\x0A";
    const CONTENTS_2: &[u8] = b"Attribute VB_Name = \"a\"\x0D\x0AabcdefgAttribute\x0D\x0A";
    let contents = decompress(CONTAINER_2, 4096).unwrap().1;
    assert_eq!(contents, CONTENTS_2);

    // CompressedContainer with first CopyToken at position 33:
    // 01 29 B0 00 41 74 74 72 69 62 75 74 00 65 20 56 42 5F 4E 61 6D 00 65 20 3D 20 22 61 22 0D 00 0A 61 62 63 64 65 66 67 02 68 06 80 0D 0A
    const CONTAINER_3: &[u8] = b"\x01\x29\xB0\x00\x41\x74\x74\x72\x69\x62\x75\x74\x00\x65\x20\x56\x42\x5F\x4E\x61\x6D\x00\x65\x20\x3D\x20\x22\x61\x22\x0D\x00\x0A\x61\x62\x63\x64\x65\x66\x67\x02\x68\x06\x80\x0D\x0A";
    const CONTENTS_3: &[u8] = b"Attribute VB_Name = \"a\"\x0D\x0AabcdefghAttribute\x0D\x0A";
    let contents = decompress(CONTAINER_3, 4096).unwrap().1;
    assert_eq!(contents, CONTENTS_3);
}

#[test]
fn decompressor_rejects_output_over_limit_before_growth() {
    use super::parser::FormatError;

    const CONTAINER: &[u8] = b"\x01\x03\xB0\x00\x41\x42\x43";
    let result = decompress(CONTAINER, 2);

    assert!(matches!(
        result,
        Err(nom::Err::Failure(FormatError::ResourceLimit))
    ));
}

#[test]
fn decompressor_accepts_exact_limit_and_uses_the_approved_dir_ceiling() {
    const CONTAINER: &[u8] = b"\x01\x03\xB0\x00\x41\x42\x43";
    assert_eq!(decompress(CONTAINER, 3).unwrap().1, b"ABC");
    assert_eq!(super::MAX_DIR_DECOMPRESSED_BYTES, super::MAX_CFB_BYTES);
    assert_eq!(super::MAX_DIR_DECOMPRESSED_BYTES, 64 * 1024 * 1024);
}

#[test]
fn decompressor_rejects_copy_token_with_invalid_offset_without_panicking() {
    const CONTAINER: &[u8] = b"\x01\x02\xB0\x01\x00\x00";
    let result = std::panic::catch_unwind(|| decompress(CONTAINER, 16));

    assert!(result.is_ok(), "malformed copy offset must not panic");
    assert!(result.unwrap().is_err(), "malformed copy offset must fail");
}

#[test]
fn decompressor_rejects_single_chunk_expansion_over_4096_without_panicking() {
    use super::parser::FormatError;

    // The first copy token fills the chunk; the second would exceed its format limit.
    const CONTAINER: &[u8] = b"\x01\x05\xB0\x06A\xFC\x0F\x00\x00";
    let result = std::panic::catch_unwind(|| decompress(CONTAINER, 8192));

    assert!(result.is_ok(), "oversized copy expansion must not panic");
    assert!(matches!(
        result.unwrap(),
        Err(nom::Err::Failure(FormatError::ResourceLimit))
    ));
}

#[test]
fn decompressor_rejects_truncated_chunk_without_panicking() {
    const CONTAINER: &[u8] = b"\x01\xFF\x3F";
    let result = std::panic::catch_unwind(|| decompress(CONTAINER, 16));

    assert!(result.is_ok(), "truncated chunk must not panic");
    assert!(result.unwrap().is_err(), "truncated chunk must fail");
}

#[test]
fn unsupported_code_page_returns_error_without_panicking() {
    let result = std::panic::catch_unwind(|| super::parser::cp_to_string(b"source", u16::MAX));

    assert!(result.is_ok(), "unsupported code page must not panic");
    assert!(result.unwrap().is_err(), "unsupported code page must fail");
}

#[test]
fn malformed_code_page_data_returns_error_without_panicking() {
    let result = std::panic::catch_unwind(|| super::parser::cp_to_string(&[0x82], 932));

    assert!(result.is_ok(), "malformed encoded source must not panic");
    assert!(
        result.unwrap().is_err(),
        "malformed encoded source must fail"
    );
}

#[test]
fn proj_info_opt_records() {
    // Version 11 of the `[MS-OVBA]` specification introduced an optional
    // `PROJECTCOMPATVERSION` record following the `PROJECTSYSKIND` record. This test
    // verifies that this addition is properly handled by the parser.
    //
    // In addition, this test verifies that the final `PROJECTCONSTANTS` is treated as
    // optional (which it should have been all along).
    //
    // The four test inputs represent the `2x2` matrix of combinations of optional
    // records.

    const INPUT_NONE_NONE: &[u8] = b"\x01\x00\x04\x00\x00\x00\x02\x00\x00\x00\
        \x02\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x14\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x03\x00\x02\x00\x00\x00\xE4\x04\
        \x04\x00\x01\x00\x00\x00\x41\
        \x05\x00\x01\x00\x00\x00\x41\x40\x00\x02\x00\x00\x00\x41\x00\
        \x06\x00\x00\x00\x00\x00\x3D\x00\x00\x00\x00\x00\
        \x07\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x08\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x09\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x0F\x00\x02\x00\x00\x00\x00\x00\
        \x13\x00\x02\x00\x00\x00\xFF\xFF\
        \x10\x00\
        \x00\x00\x00\x00";
    let res = parse_project_information(INPUT_NONE_NONE);
    assert!(res.is_ok());
    let res = res.unwrap();
    assert!(res.1.information.constants.is_none());

    const INPUT_NONE_SOME: &[u8] = b"\x01\x00\x04\x00\x00\x00\x02\x00\x00\x00\
        \x02\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x14\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x03\x00\x02\x00\x00\x00\xE4\x04\
        \x04\x00\x01\x00\x00\x00\x41\
        \x05\x00\x01\x00\x00\x00\x41\x40\x00\x02\x00\x00\x00\x41\x00\
        \x06\x00\x00\x00\x00\x00\x3D\x00\x00\x00\x00\x00\
        \x07\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x08\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x09\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x0C\x00\x00\x00\x00\x00\x3C\x00\x00\x00\x00\x00\
        \x0F\x00\x02\x00\x00\x00\x00\x00\
        \x13\x00\x02\x00\x00\x00\xFF\xFF\
        \x10\x00\
        \x00\x00\x00\x00";
    let res = parse_project_information(INPUT_NONE_SOME);
    assert!(res.is_ok());
    let res = res.unwrap();
    assert!(res.1.information.constants.is_some());

    const INPUT_SOME_NONE: &[u8] = b"\x01\x00\x04\x00\x00\x00\x02\x00\x00\x00\
        \x4A\x00\x04\x00\x00\x00\x01\x02\x03\x04\
        \x02\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x14\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x03\x00\x02\x00\x00\x00\xE4\x04\
        \x04\x00\x01\x00\x00\x00\x41\
        \x05\x00\x01\x00\x00\x00\x41\x40\x00\x02\x00\x00\x00\x41\x00\
        \x06\x00\x00\x00\x00\x00\x3D\x00\x00\x00\x00\x00\
        \x07\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x08\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x09\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x0F\x00\x02\x00\x00\x00\x00\x00\
        \x13\x00\x02\x00\x00\x00\xFF\xFF\
        \x10\x00\
        \x00\x00\x00\x00";
    let res = parse_project_information(INPUT_SOME_NONE);
    assert!(res.is_ok());

    const INPUT_SOME_SOME: &[u8] = b"\x01\x00\x04\x00\x00\x00\x02\x00\x00\x00\
        \x4A\x00\x04\x00\x00\x00\x01\x02\x03\x04\
        \x02\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x14\x00\x04\x00\x00\x00\x09\x04\x00\x00\
        \x03\x00\x02\x00\x00\x00\xE4\x04\
        \x04\x00\x01\x00\x00\x00\x41\
        \x05\x00\x01\x00\x00\x00\x41\x40\x00\x02\x00\x00\x00\x41\x00\
        \x06\x00\x00\x00\x00\x00\x3D\x00\x00\x00\x00\x00\
        \x07\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x08\x00\x04\x00\x00\x00\x00\x00\x00\x00\
        \x09\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x0C\x00\x00\x00\x00\x00\x3C\x00\x00\x00\x00\x00\
        \x0F\x00\x02\x00\x00\x00\x00\x00\
        \x13\x00\x02\x00\x00\x00\xFF\xFF\
        \x10\x00\
        \x00\x00\x00\x00";
    let res = parse_project_information(INPUT_SOME_SOME);
    assert!(res.is_ok());
}
