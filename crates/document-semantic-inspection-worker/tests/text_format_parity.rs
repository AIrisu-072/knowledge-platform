use document_semantic_inspection_core::{FormatId, SemanticFingerprint};
use document_semantic_inspection_worker::{
    AdapterProfile, CsvAdapter, HtmlAdapter, SemanticAdapter, TextAdapter, WorkerFailureCode,
};

const TXT_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/txt/base.txt");
const TXT_CRLF: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/txt/crlf.txt");
const TXT_NFD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/txt/unicode-nfd.txt"
);
const TXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/txt/text-change.txt"
);
const TXT_AMBIGUOUS: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/txt/ambiguous.txt");

const CSV_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/csv/base.csv");
const CSV_QUOTE_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/quote-noise.csv"
);
const CSV_CELL_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/cell-change.csv"
);
const CSV_ROW_CHANGE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/csv/row-change.csv");
const CSV_INCONSISTENT: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/inconsistent.csv"
);
const CSV_AMBIGUOUS: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/csv/delimiter-ambiguous.csv"
);

const HTML_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/base.html");
const HTML_NOISE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/noise.html");
const HTML_TEXT_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/text-change.html"
);
const HTML_LINK_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/link-change.html"
);
const HTML_IMAGE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/html/image-change.html"
);
const HTML_JS_ONLY: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/html/js-only.html");

fn fingerprint(
    adapter: &dyn SemanticAdapter,
    bytes: &[u8],
    profile: &AdapterProfile,
) -> SemanticFingerprint {
    adapter
        .inspect(bytes, profile)
        .expect("fixture should inspect")
        .semantic_fingerprint()
}

#[test]
fn txt_preserves_the_poc_qualified_semantic_relations() {
    let adapter = TextAdapter;
    let profile = AdapterProfile::default();

    assert_eq!(
        fingerprint(&adapter, TXT_BASE, &profile),
        fingerprint(&adapter, TXT_CRLF, &profile)
    );
    assert_eq!(
        fingerprint(&adapter, TXT_BASE, &profile),
        fingerprint(&adapter, TXT_NFD, &profile)
    );
    assert_ne!(
        fingerprint(&adapter, TXT_BASE, &profile),
        fingerprint(&adapter, TXT_CHANGE, &profile)
    );

    let ambiguous = AdapterProfile::default().with_text_encoding("ambiguous");
    let error = adapter.inspect(TXT_AMBIGUOUS, &ambiguous).unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::SemanticExtractionFailed);
}

#[test]
fn csv_preserves_tabular_semantics_and_fails_closed() {
    let adapter = CsvAdapter;
    let comma = AdapterProfile::default().with_csv_delimiter(b',');

    assert_eq!(
        fingerprint(&adapter, CSV_BASE, &comma),
        fingerprint(&adapter, CSV_QUOTE_NOISE, &comma)
    );
    assert_ne!(
        fingerprint(&adapter, CSV_BASE, &comma),
        fingerprint(&adapter, CSV_CELL_CHANGE, &comma)
    );
    assert_ne!(
        fingerprint(&adapter, CSV_BASE, &comma),
        fingerprint(&adapter, CSV_ROW_CHANGE, &comma)
    );

    let error = adapter.inspect(CSV_INCONSISTENT, &comma).unwrap_err();
    assert_eq!(error.code(), WorkerFailureCode::SemanticExtractionFailed);

    let error = adapter
        .inspect(CSV_BASE, &AdapterProfile::default())
        .unwrap_err();
    assert_eq!(
        error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );

    let error = adapter
        .inspect(CSV_AMBIGUOUS, &AdapterProfile::default())
        .unwrap_err();
    assert_eq!(
        error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn html_preserves_visible_structure_targets_and_noise_invariance() {
    let adapter = HtmlAdapter;
    let profile = AdapterProfile::default();

    assert_eq!(
        fingerprint(&adapter, HTML_BASE, &profile),
        fingerprint(&adapter, HTML_NOISE, &profile)
    );
    assert_ne!(
        fingerprint(&adapter, HTML_BASE, &profile),
        fingerprint(&adapter, HTML_TEXT_CHANGE, &profile)
    );
    assert_ne!(
        fingerprint(&adapter, HTML_BASE, &profile),
        fingerprint(&adapter, HTML_LINK_CHANGE, &profile)
    );
    assert_ne!(
        fingerprint(&adapter, HTML_BASE, &profile),
        fingerprint(&adapter, HTML_IMAGE_CHANGE, &profile)
    );

    let script_required = AdapterProfile::default().with_html_script_required(true);
    let error = adapter.inspect(HTML_JS_ONLY, &script_required).unwrap_err();
    assert_eq!(
        error.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}

#[test]
fn text_format_adapters_report_their_frozen_formats() {
    assert_eq!(TextAdapter.format(), FormatId::Txt);
    assert_eq!(CsvAdapter.format(), FormatId::Csv);
    assert_eq!(HtmlAdapter.format(), FormatId::Html);
}
