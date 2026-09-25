use document_semantic_inspection_poc::{
    aggregate_promotion_gates, AdapterRegistry, CsvAdapter, DocxAdapter, ExternalGateEvidence,
    FixtureCase, FixtureGateCounts, FixtureManifest, FormatId, GateCount, HtmlAdapter,
    InspectionAdapter, InspectionProfile, PdfAdapter, PptxAdapter, SpreadsheetAdapter, TextAdapter,
    fingerprint, run_case, verify_manifest, write_reports,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "verify".to_owned());

    if command == "self-test-hang" {
        std::thread::sleep(Duration::from_secs(60));
        return Ok(());
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = root.join("fixtures");
    let manifest = FixtureManifest::from_path(&fixture_root.join("manifest.json"))?;

    match command.as_str() {
        "verify" => verify(&root, &fixture_root, &manifest),
        "snapshot" => {
            let selector = args.next().ok_or("snapshot requires a format or 'all'")?;
            snapshot(&manifest, &selector)
        }
        "inspect-case" => {
            let case_id = args.next().ok_or("inspect-case requires a case id")?;
            inspect_case(&manifest, &case_id)
        }
        "inspect-file" => {
            let format = args.next().ok_or("inspect-file requires a format")?;
            let path = args.next().ok_or("inspect-file requires a path")?;
            inspect_file(&format, std::path::Path::new(&path))
        }
        "report" => {
            let flag = args.next().ok_or("report requires --format json")?;
            let format = args.next().ok_or("report requires --format json")?;
            if flag != "--format" || format != "json" {
                return Err("only 'report --format json' is supported".into());
            }
            final_report(&root, &fixture_root, &manifest)
        }
        _ => Err(format!(
            "unsupported command {command:?}; expected 'verify', 'snapshot', 'inspect-case', 'inspect-file', 'report', or 'self-test-hang'"
        )
        .into()),
    }
}

fn default_registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.insert(Box::new(TextAdapter));
    registry.insert(Box::new(CsvAdapter));
    registry.insert(Box::new(HtmlAdapter));
    registry.insert(Box::new(DocxAdapter));
    registry.insert(Box::new(SpreadsheetAdapter::XLSX));
    registry.insert(Box::new(SpreadsheetAdapter::XLSM));
    registry.insert(Box::new(PptxAdapter));
    registry.insert(Box::new(PdfAdapter));
    registry
}

fn verify(
    root: &std::path::Path,
    fixture_root: &std::path::Path,
    manifest: &FixtureManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = default_registry();
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

fn adapter_for(format: FormatId) -> Box<dyn InspectionAdapter> {
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

fn expected_error(case: &FixtureCase) -> bool {
    serde_json::to_value(&case.expected)
        .ok()
        .and_then(|value| value.get("kind").and_then(serde_json::Value::as_str).map(str::to_owned))
        .as_deref()
        == Some("error")
}

fn format_selector(format: FormatId) -> &'static str {
    match format {
        FormatId::Txt => "txt",
        FormatId::Csv => "csv",
        FormatId::Html => "html",
        FormatId::Docx => "docx",
        FormatId::Xlsx => "xlsx",
        FormatId::Xlsm => "xlsm",
        FormatId::Pptx => "pptx",
        FormatId::Pdf => "pdf",
    }
}

fn normalized_snapshot(case: &FixtureCase) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let adapter = adapter_for(case.format);
    let result = run_case(case, adapter.as_ref())?;
    Ok(serde_json::json!({
        "semantic_fingerprint": hex::encode(result.semantic_fingerprint),
        "semantic_projection": hex::encode(&result.output.semantic_projection),
        "capabilities": result.output.capabilities,
        "editorial": result.output.editorial,
        "external_dependencies": result.output.external_dependencies,
        "signatures": result.output.signatures,
        "projection_hash": hex::encode(fingerprint(&result.output.semantic_projection)),
    }))
}

fn snapshot(
    manifest: &FixtureManifest,
    requested: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let valid_selector = requested == "all"
        || matches!(
            requested,
            "txt" | "csv" | "html" | "docx" | "xlsx" | "xlsm" | "pptx" | "pdf"
        );
    if !valid_selector {
        return Err(format!("unsupported snapshot selector {requested:?}").into());
    }

    let mut snapshot = BTreeMap::new();
    for case in &manifest.cases {
        if expected_error(case) {
            continue;
        }
        if requested != "all" && requested != format_selector(case.format) {
            continue;
        }
        snapshot.insert(case.id.clone(), normalized_snapshot(case)?);
    }

    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}

fn inspect_case(
    manifest: &FixtureManifest,
    requested_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(case) = manifest.cases.iter().find(|case| case.id == requested_id) else {
        eprintln!("inspection_error:unknown_case");
        std::process::exit(2);
    };
    let adapter = adapter_for(case.format);
    match run_case(case, adapter.as_ref()) {
        Ok(result) => {
            let snapshot = serde_json::json!({
                "semantic_fingerprint": hex::encode(result.semantic_fingerprint),
                "capabilities": result.output.capabilities,
                "editorial": result.output.editorial,
                "external_dependencies": result.output.external_dependencies,
                "signatures": result.output.signatures,
            });
            println!("{}", serde_json::to_string(&snapshot)?);
            Ok(())
        }
        Err(error) => {
            let code = serde_json::to_string(&error.code())?;
            eprintln!("inspection_error:{}", code.trim_matches('"'));
            std::process::exit(2);
        }
    }
}

fn parse_format_selector(value: &str) -> Option<FormatId> {
    match value {
        "txt" => Some(FormatId::Txt),
        "csv" => Some(FormatId::Csv),
        "html" => Some(FormatId::Html),
        "docx" => Some(FormatId::Docx),
        "xlsx" => Some(FormatId::Xlsx),
        "xlsm" => Some(FormatId::Xlsm),
        "pptx" => Some(FormatId::Pptx),
        "pdf" => Some(FormatId::Pdf),
        _ => None,
    }
}

fn inspect_file(
    format: &str,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(format) = parse_format_selector(format) else {
        eprintln!("inspection_error:unsupported_document_format");
        std::process::exit(2);
    };
    let bytes = std::fs::read(path)?;
    let adapter = adapter_for(format);
    match adapter.inspect(&bytes, &InspectionProfile::default()) {
        Ok(output) => {
            let snapshot = serde_json::json!({
                "semantic_fingerprint": hex::encode(fingerprint(&output.semantic_projection)),
                "capabilities": output.capabilities,
                "editorial": output.editorial,
                "external_dependencies": output.external_dependencies,
                "signatures": output.signatures,
            });
            println!("{}", serde_json::to_string(&snapshot)?);
            Ok(())
        }
        Err(error) => {
            let code = serde_json::to_string(&error.code())?;
            eprintln!("inspection_error:{}", code.trim_matches('"'));
            std::process::exit(2);
        }
    }
}

fn final_report(
    root: &std::path::Path,
    fixture_root: &std::path::Path,
    manifest: &FixtureManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = default_registry();
    let verification = verify_manifest(manifest, fixture_root, &registry);
    let gates_dir = root.join("target").join("dsi-poc").join("gates");
    let runtime_passed = gates_dir.join("runtime-security.ok").is_file();
    let license_passed = gates_dir.join("license-dependency.ok").is_file();

    let formats: BTreeSet<FormatId> = manifest.cases.iter().map(|case| case.format).collect();
    let mut determinism = BTreeMap::new();
    let mut security_resource = BTreeMap::new();
    for format in &formats {
        let required = manifest
            .cases
            .iter()
            .filter(|case| case.format == *format && !expected_error(case))
            .count();
        determinism.insert(
            *format,
            GateCount {
                passed: if runtime_passed { required } else { 0 },
                required,
            },
        );
        security_resource.insert(*format, runtime_passed);
    }

    let xlsm_supplemental = FixtureGateCounts {
        semantic_change: GateCount {
            passed: if runtime_passed { 1 } else { 0 },
            required: 1,
        },
        noise_invariance: GateCount {
            passed: if runtime_passed { 2 } else { 0 },
            required: 2,
        },
        editorial: GateCount::default(),
        fail_closed: GateCount {
            passed: if runtime_passed { 1 } else { 0 },
            required: 1,
        },
    };

    let external = ExternalGateEvidence {
        supplemental_fixtures: BTreeMap::from([(FormatId::Xlsm, xlsm_supplemental)]),
        determinism,
        security_resource,
        license_dependency: license_passed,
    };
    let gates = aggregate_promotion_gates(&verification, &external);

    let required_formats = [
        FormatId::Txt,
        FormatId::Csv,
        FormatId::Html,
        FormatId::Docx,
        FormatId::Xlsx,
        FormatId::Xlsm,
        FormatId::Pptx,
        FormatId::Pdf,
    ];

    let mut format_outcomes = BTreeMap::new();
    let mut saw_fail = false;
    let mut saw_blocked = false;

    for format in required_formats {
        let name = format_selector(format);
        let outcome = match gates.get(&format) {
            Some(gate)
                if !gate.semantic_change.complete()
                    || !gate.noise_invariance.complete()
                    || !gate.editorial.complete()
                    || !gate.fail_closed.complete() =>
            {
                saw_fail = true;
                "FAIL"
            }
            Some(gate) if gate.promotion_eligible => "PASS",
            Some(_) | None => {
                saw_blocked = true;
                "BLOCKED"
            }
        };
        format_outcomes.insert(name.to_owned(), outcome);
    }

    let overall = if saw_fail {
        "FAIL"
    } else if saw_blocked {
        "BLOCKED"
    } else {
        "PASS"
    };

    let machine = serde_json::json!({
        "schema_version": 1,
        "overall": overall,
        "formats": format_outcomes,
        "promotion_gates": gates.values().collect::<Vec<_>>(),
        "verification": verification,
        "gate_provenance": {
            "runtime_security_marker": runtime_passed,
            "license_dependency_marker": license_passed,
            "sandbox_wrapper": "scripts/run-sandboxed-case.sh",
            "sandbox_limits": {
                "cpu_seconds": 8,
                "file_blocks": 2048,
                "linux_virtual_memory_kib": 2097152,
            },
        },
    });
    println!("{}", serde_json::to_string_pretty(&machine)?);

    if overall != "PASS" {
        std::process::exit(1);
    }
    Ok(())
}
