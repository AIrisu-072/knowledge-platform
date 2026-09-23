use document_semantic_inspection_poc::{
    AdapterRegistry, CsvAdapter, DocxAdapter, FixtureManifest, FormatId, HtmlAdapter,
    PptxAdapter, SpreadsheetAdapter, TextAdapter,
    fingerprint, run_case, verify_manifest, write_reports,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "verify".to_owned());

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = root.join("fixtures");
    let manifest = FixtureManifest::from_path(&fixture_root.join("manifest.json"))?;

    match command.as_str() {
        "verify" => verify(&root, &fixture_root, &manifest),
        "snapshot" => {
            let format = args
                .next()
                .ok_or("snapshot requires a format argument")?;
            snapshot(&manifest, &format)
        }
        _ => Err(format!(
            "unsupported command {command:?}; expected 'verify' or 'snapshot'"
        )
        .into()),
    }
}

fn verify(
    root: &std::path::Path,
    fixture_root: &std::path::Path,
    manifest: &FixtureManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = AdapterRegistry::new();
    registry.insert(Box::new(TextAdapter));
    registry.insert(Box::new(CsvAdapter));
    registry.insert(Box::new(HtmlAdapter));
    registry.insert(Box::new(DocxAdapter));
    registry.insert(Box::new(SpreadsheetAdapter::XLSX));
    registry.insert(Box::new(SpreadsheetAdapter::XLSM));
    registry.insert(Box::new(PptxAdapter));

    let report = verify_manifest(manifest, fixture_root, &registry);
    let output_dir = root.join("target").join("dsi-poc");
    write_reports(&report, &output_dir)?;

    if !report.passed {
        eprintln!("Document Semantic Inspection PoC verification failed");
        std::process::exit(1);
    }

    println!(
        "Document Semantic Inspection PoC verification passed ({} cases)",
        report.cases.len()
    );
    Ok(())
}

fn snapshot(
    manifest: &FixtureManifest,
    requested_format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if requested_format != "docx" {
        return Err(format!("unsupported snapshot format {requested_format:?}").into());
    }

    let mut snapshot = BTreeMap::new();
    for case in &manifest.cases {
        if case.format != FormatId::Docx {
            continue;
        }

        let expected = serde_json::to_value(&case.expected)?;
        if expected.get("kind").and_then(serde_json::Value::as_str) == Some("error") {
            continue;
        }

        let result = run_case(case, &DocxAdapter)?;
        snapshot.insert(
            case.id.clone(),
            serde_json::json!({
                "semantic_fingerprint": hex::encode(result.semantic_fingerprint),
                "semantic_projection": hex::encode(&result.output.semantic_projection),
                "capabilities": result.output.capabilities,
                "editorial": result.output.editorial,
                "external_dependencies": result.output.external_dependencies,
                "signatures": result.output.signatures,
                "diagnostics": result.output.diagnostics,
                "projection_hash": hex::encode(fingerprint(&result.output.semantic_projection)),
            }),
        );
    }

    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}
