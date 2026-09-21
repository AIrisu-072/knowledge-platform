use document_semantic_inspection_poc::{
    AdapterRegistry, CsvAdapter, DocxAdapter, FixtureManifest, HtmlAdapter, TextAdapter, verify_manifest,
    write_reports,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "verify".to_owned());
    if command != "verify" {
        return Err(format!("unsupported command {command:?}; expected 'verify'").into());
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = root.join("fixtures");
    let manifest = FixtureManifest::from_path(&fixture_root.join("manifest.json"))?;

    let mut registry = AdapterRegistry::new();
    registry.insert(Box::new(TextAdapter));
    registry.insert(Box::new(CsvAdapter));
    registry.insert(Box::new(HtmlAdapter));
    registry.insert(Box::new(DocxAdapter));

    let report = verify_manifest(&manifest, &fixture_root, &registry);
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
