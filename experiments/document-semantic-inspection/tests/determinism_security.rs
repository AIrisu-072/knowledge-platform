mod support;
use document_semantic_inspection_poc::{
    CsvAdapter, DocxAdapter, FixtureCase, FixtureManifest, FormatId, HtmlAdapter, InspectionAdapter,
    PdfAdapter, PptxAdapter, SpreadsheetAdapter, TextAdapter, run_case,
};
use serde_json::{Value, json};
use std::io::{Cursor, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use support::spreadsheetml::mutate_vba_module;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("manifest.json"),
    )
    .expect("manifest")
}

fn adapter(format: FormatId) -> Box<dyn InspectionAdapter> {
    match format {
        FormatId::Txt => Box::new(TextAdapter),
        FormatId::Csv => Box::new(CsvAdapter),
        FormatId::Html => Box::new(HtmlAdapter),
        FormatId::Docx => Box::new(DocxAdapter),
        FormatId::Xlsx => Box::new(SpreadsheetAdapter::XLSX),
        FormatId::Xlsm => Box::new(SpreadsheetAdapter::XLSM),
        FormatId::Pptx => Box::new(PptxAdapter),
        FormatId::Pdf => Box::new(PdfAdapter),
    }
}

fn expects_error(case: &FixtureCase) -> bool {
    serde_json::to_value(&case.expected)
        .ok()
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        == Some("error")
}

fn snapshot_case(case: &FixtureCase) -> Value {
    let adapter = adapter(case.format);
    let result = run_case(case, adapter.as_ref())
        .unwrap_or_else(|error| panic!("{} should be a successful fixture: {error}", case.id));
    json!({
        "semantic_fingerprint": hex::encode(result.semantic_fingerprint),
        "semantic_projection": hex::encode(result.output.semantic_projection),
        "capabilities": result.output.capabilities,
        "editorial": result.output.editorial,
        "external_dependencies": result.output.external_dependencies,
        "signatures": result.output.signatures,
    })
}

#[test]
fn every_successful_fixture_is_deterministic_for_twenty_in_process_runs() {
    for case in manifest().cases.into_iter().filter(|case| !expects_error(case)) {
        let baseline = snapshot_case(&case);
        for repetition in 1..20 {
            assert_eq!(
                snapshot_case(&case),
                baseline,
                "{} changed on in-process repetition {repetition}",
                case.id
            );
        }
    }
}

fn child_snapshot(tz: &str, lang: &str) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_dsi-poc"))
        .args(["snapshot", "all"])
        .env("TZ", tz)
        .env("LANG", lang)
        .env("LC_ALL", lang)
        .output()
        .expect("run child snapshot");
    assert!(
        output.status.success(),
        "child snapshot failed for TZ={tz} LANG={lang}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn five_fresh_processes_are_identical_across_timezone_and_locale_variation() {
    let variants = [
        ("UTC", "C"),
        ("Asia/Tokyo", "C"),
        ("UTC", "en_US.UTF-8"),
        ("Asia/Tokyo", "ja_JP.UTF-8"),
        ("America/New_York", "C"),
    ];
    let baseline = child_snapshot(variants[0].0, variants[0].1);
    for (tz, lang) in variants.into_iter().skip(1) {
        assert_eq!(
            child_snapshot(tz, lang),
            baseline,
            "fresh-process snapshot changed for TZ={tz} LANG={lang}"
        );
    }
}

struct ChildResult {
    status_success: bool,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_child_with_timeout(args: &[&str], timeout: Duration, output_dir: &std::path::Path) -> ChildResult {
    let sandbox = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("run-sandboxed-case.sh");
    let mut command = Command::new("bash");
    command.arg(sandbox).arg(env!("CARGO_BIN_EXE_dsi-poc")).args(args);
    let mut child = command
        .env("DSI_POC_OUTPUT_DIR", output_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sandbox child");
    let started = Instant::now();

    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            child
                .stdout
                .take()
                .expect("child stdout")
                .read_to_end(&mut stdout)
                .expect("read stdout");
            child
                .stderr
                .take()
                .expect("child stderr")
                .read_to_end(&mut stderr)
                .expect("read stderr");
            return ChildResult {
                status_success: status.success(),
                timed_out: false,
                stdout,
                stderr,
            };
        }

        if started.elapsed() >= timeout {
            child.kill().expect("kill timed-out child");
            let _ = child.wait();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut handle) = child.stdout.take() {
                let _ = handle.read_to_end(&mut stdout);
            }
            if let Some(mut handle) = child.stderr.take() {
                let _ = handle.read_to_end(&mut stderr);
            }
            return ChildResult {
                status_success: false,
                timed_out: true,
                stdout,
                stderr,
            };
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn hostile_fixtures_fail_in_child_process_without_partial_output_or_body_leakage() {
    let cases = [
        "txt/ambiguous-decode",
        "csv/inconsistent",
        "html/js-only",
        "docx/deep-ooxml",
        "docx/malformed",
        "xlsx/unknown-semantic-part",
        "pptx/unknown-semantic-part",
        "pdf/broken-xref",
        "pdf/ambiguous-read-order",
    ];

    for id in cases {
        let temp = tempfile::tempdir().expect("temp output");
        let result = run_child_with_timeout(
            &["inspect-case", id],
            Duration::from_secs(10),
            temp.path(),
        );
        assert!(!result.timed_out, "{id} unexpectedly timed out");
        assert!(!result.status_success, "{id} unexpectedly succeeded");
        assert!(result.stdout.is_empty(), "{id} emitted partial success output");

        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.starts_with("inspection_error:"), "{id}: {stderr}");
        for body_fragment in ["Base text", "semantic-value", "DSI Signed PDF"] {
            assert!(
                !stderr.contains(body_fragment),
                "{id} leaked fixture body fragment {body_fragment:?}: {stderr}"
            );
        }

        assert_eq!(
            std::fs::read_dir(temp.path()).expect("output dir").count(),
            0,
            "{id} left a partial result file"
        );
    }
}

fn oversized_docx_bytes() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer
        .start_file("oversized.bin", options)
        .expect("start oversized entry");
    writer
        .write_all(&vec![b'X'; 8 * 1024 * 1024 + 1])
        .expect("write oversized entry");
    writer.finish().expect("finish oversized zip").into_inner()
}

#[test]
fn xlsm_invalid_vba_and_oversized_docx_fail_in_sandboxed_child_process() {
    const VBA_BASE_SOURCE: &str =
        "Attribute VB_Name = \"testVBA\"\r\nPublic Sub test()\r\n    MsgBox \"Hello from vba!\"\r\nEnd Sub\r\n";

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let xlsm_temp = tempfile::tempdir().expect("XLSM temp");
    let output_dir = xlsm_temp.path().join("output");
    std::fs::create_dir(&output_dir).expect("XLSM output dir");
    let seed = std::fs::read(root.join("fixtures/xlsm/calamine-vba.xlsm")).expect("XLSM seed");
    let invalid = mutate_vba_module(
        &seed,
        "testVBA",
        &VBA_BASE_SOURCE.replace("Public Sub test()", "Public Sub test("),
    );
    let xlsm_path = xlsm_temp.path().join("invalid.xlsm");
    std::fs::write(&xlsm_path, invalid).expect("write invalid XLSM");
    let xlsm_path_text = xlsm_path.to_string_lossy().into_owned();
    let result = run_child_with_timeout(
        &["inspect-file", "xlsm", &xlsm_path_text],
        Duration::from_secs(10),
        &output_dir,
    );
    assert!(!result.timed_out, "invalid XLSM unexpectedly timed out");
    assert!(!result.status_success, "invalid XLSM unexpectedly succeeded");
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.starts_with("inspection_error:semantic_extraction_failed"), "{stderr}");
    assert!(!stderr.contains("Hello from vba!"), "XLSM source leaked: {stderr}");
    assert_eq!(std::fs::read_dir(&output_dir).expect("XLSM output").count(), 0);

    let docx_temp = tempfile::tempdir().expect("DOCX temp");
    let output_dir = docx_temp.path().join("output");
    std::fs::create_dir(&output_dir).expect("DOCX output dir");
    let docx_path = docx_temp.path().join("oversized.docx");
    std::fs::write(&docx_path, oversized_docx_bytes()).expect("write oversized DOCX");
    let docx_path_text = docx_path.to_string_lossy().into_owned();
    let result = run_child_with_timeout(
        &["inspect-file", "docx", &docx_path_text],
        Duration::from_secs(10),
        &output_dir,
    );
    assert!(!result.timed_out, "oversized DOCX unexpectedly timed out");
    assert!(!result.status_success, "oversized DOCX unexpectedly succeeded");
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.starts_with("inspection_error:inspection_resource_limit_exceeded"),
        "{stderr}"
    );
    assert_eq!(std::fs::read_dir(&output_dir).expect("DOCX output").count(), 0);
}

#[test]
fn timeout_controller_terminates_a_stuck_sandbox_process() {
    let temp = tempfile::tempdir().expect("temp output");
    let result = run_child_with_timeout(
        &["self-test-hang"],
        Duration::from_millis(200),
        temp.path(),
    );
    assert!(result.timed_out, "timeout controller did not terminate child");
    assert!(!result.status_success);
    assert!(result.stdout.is_empty());
    assert_eq!(
        std::fs::read_dir(temp.path()).expect("output dir").count(),
        0,
        "timed-out child left a partial result file"
    );
}
