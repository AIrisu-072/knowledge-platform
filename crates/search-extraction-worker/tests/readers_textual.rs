//! P1-I03 Text/CSV/HTML readers: golden Unit text/kind/locator and fail-closed scope.

mod support;

use search_core::knowledge_unit::{BudgetKey, FormatId, FormatSettings, NativeLocator, UnitKind};
use search_extraction_core::{
    BodyCoverage, CoverageReason, PermanentFailureCode, ReaderFailure, WorkerReport,
};
use search_extraction_worker::readers::extract_for_test;

fn read(
    format: FormatId,
    settings: &FormatSettings,
    raw: &[u8],
) -> Result<WorkerReport, ReaderFailure> {
    extract_for_test(format, settings, raw, &mut support::meter(format))
}

fn units(report: &WorkerReport) -> Vec<(UnitKind, &str, &NativeLocator)> {
    report
        .fragments
        .iter()
        .map(|fragment| (fragment.kind, fragment.text.as_str(), &fragment.locator))
        .collect()
}

fn text_locator(line: u32) -> NativeLocator {
    NativeLocator::Text {
        line_start: line,
        line_end: line + 1,
    }
}

#[test]
fn text_bom_cp932_and_line_boundaries() {
    let raw = "\u{feff}東京\r\n\r\n同文。\r".as_bytes();
    let report = read(FormatId::Text, &support::settings(FormatId::Text), raw).unwrap();
    assert_eq!(report.coverage, BodyCoverage::Supported);
    assert_eq!(
        units(&report),
        vec![
            (UnitKind::PlainText, "東京", &text_locator(0)),
            (UnitKind::PlainText, "同文。", &text_locator(2)),
        ]
    );
    assert_eq!(report.scope_items, 4);
    support::assert_valid(FormatId::Text, &report);

    // 東京\r\n同文。 in Windows-31J.
    let cp932 = [
        0x93, 0x8c, 0x8b, 0x9e, b'\r', b'\n', 0x93, 0xaf, 0x95, 0xb6, 0x81, 0x42,
    ];
    let settings = FormatSettings::Text {
        charset: "windows-31j".into(),
    };
    let report = read(FormatId::Text, &settings, &cp932).unwrap();
    assert_eq!(
        units(&report),
        vec![
            (UnitKind::PlainText, "東京", &text_locator(0)),
            (UnitKind::PlainText, "同文。", &text_locator(1)),
        ]
    );

    for (charset, raw) in [
        ("utf-8", &[0xff, 0xfe][..]),
        ("windows-31j", &[0x81, 0xff][..]),
    ] {
        let settings = FormatSettings::Text {
            charset: charset.into(),
        };
        assert_eq!(
            read(FormatId::Text, &settings, raw),
            Err(ReaderFailure::Unsupported(
                CoverageReason::UnsupportedEncoding
            ))
        );
    }
    let unknown = FormatSettings::Text {
        charset: "iso-2022-jp".into(),
    };
    assert_eq!(
        read(FormatId::Text, &unknown, b"plain"),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedEncoding
        ))
    );
}

#[test]
fn csv_quoted_newline_and_ambiguous_dialect() {
    let raw = "a,\"b\r\nc\"\r\n東京,同文。\r\n".as_bytes();
    let report = read(FormatId::Csv, &support::settings(FormatId::Csv), raw).unwrap();
    let csv = |record, field| NativeLocator::Csv { record, field };
    assert_eq!(
        units(&report),
        vec![
            (UnitKind::CsvField, "a", &csv(0, 0)),
            (UnitKind::CsvField, "b\nc", &csv(0, 1)),
            (UnitKind::CsvField, "東京", &csv(1, 0)),
            (UnitKind::CsvField, "同文。", &csv(1, 1)),
        ]
    );
    assert_eq!(report.scope_items, 2);
    support::assert_valid(FormatId::Csv, &report);

    assert_eq!(
        read(
            FormatId::Csv,
            &support::settings(FormatId::Csv),
            b"a;b\n1;2\n"
        ),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedDialect
        ))
    );
    let tab = FormatSettings::Csv {
        charset: "utf-8".into(),
        delimiter: b'\t',
        quote: b'"',
    };
    assert_eq!(
        read(FormatId::Csv, &tab, b"a\tb\n"),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedDialect
        ))
    );
    assert_eq!(
        read(
            FormatId::Csv,
            &support::settings(FormatId::Csv),
            b"a,b\nc\n"
        ),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::CorruptDocument
        ))
    );
    let mut meter = support::meter_with(FormatId::Csv, BudgetKey::CsvFieldBytes, 4);
    assert_eq!(
        extract_for_test(
            FormatId::Csv,
            &support::settings(FormatId::Csv),
            b"short,longer-than-four\n",
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
}

#[test]
fn html_dom_path_hidden_and_script_scope() {
    let raw = "<html><head><title>x</title></head><body><h1>東京</h1>\n<p>同文。<b>強調</b></p></body></html>";
    let report = read(FormatId::Html, &FormatSettings::None, raw.as_bytes()).unwrap();
    let html = |path: &[u32]| NativeLocator::Html {
        text_node_path: path.to_vec(),
    };
    assert_eq!(
        units(&report),
        vec![
            (UnitKind::Heading, "東京", &html(&[0, 0])),
            (UnitKind::HtmlText, "同文。", &html(&[2, 0])),
            (UnitKind::HtmlText, "強調", &html(&[2, 1, 0])),
        ]
    );
    support::assert_valid(FormatId::Html, &report);

    for dynamic in [
        "<html><body><p>a</p><script>document.write('b')</script></body></html>",
        "<html><body><p hidden>a</p><p>b</p></body></html>",
        "<html><body><p style=\"display:none\">a</p></body></html>",
        "<html><head><link rel=\"stylesheet\" href=\"x.css\"></head><body><p>a</p></body></html>",
    ] {
        let result = read(FormatId::Html, &FormatSettings::None, dynamic.as_bytes());
        if dynamic.contains("<link") {
            // The stylesheet link is outside <body>; only body scope is traversed.
            assert!(result.is_ok(), "{dynamic}");
        } else {
            assert_eq!(
                result,
                Err(ReaderFailure::Unsupported(
                    CoverageReason::DynamicVisibility
                )),
                "{dynamic}"
            );
        }
    }
    let mut meter = support::meter_with(FormatId::Html, BudgetKey::HtmlNodes, 2);
    assert_eq!(
        extract_for_test(
            FormatId::Html,
            &FormatSettings::None,
            b"<html><body><p>a</p><p>b</p></body></html>",
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
    assert_eq!(
        read(FormatId::Html, &FormatSettings::None, &[0xff, b'<']),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedEncoding
        ))
    );
}

#[test]
fn utf8_nfc_literal_not_casefolded() {
    // か + U+3099 is normalized to the composed が; ASCII case is preserved.
    let raw = "か\u{3099}ABC\r\nｶﾞ";
    let report = read(
        FormatId::Text,
        &support::settings(FormatId::Text),
        raw.as_bytes(),
    )
    .unwrap();
    assert_eq!(report.fragments[0].text, "がABC");
    // Compatibility characters are not folded: half-width kana stays as written.
    assert_eq!(report.fragments[1].text, "ｶﾞ");
}

#[test]
fn unit_budget_is_enforced_before_publication() {
    let mut meter = support::meter_with(FormatId::Text, BudgetKey::Units, 1);
    assert_eq!(
        extract_for_test(
            FormatId::Text,
            &support::settings(FormatId::Text),
            b"one\ntwo\n",
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
    let mut meter = support::meter_with(FormatId::Text, BudgetKey::UnitUtf8Bytes, 3);
    assert_eq!(
        extract_for_test(
            FormatId::Text,
            &support::settings(FormatId::Text),
            b"four\n",
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
}
