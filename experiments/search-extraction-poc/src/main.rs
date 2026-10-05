mod readers;

use readers::{KnownOmission, Unit, inspect};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    time::Instant,
};

#[cfg(target_os = "macos")]
const PDFIUM_SHA256: &str = "1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7";
#[cfg(target_os = "linux")]
const PDFIUM_SHA256: &str = "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64";

#[derive(Deserialize)]
struct Request {
    rows: Vec<Row>,
    root: PathBuf,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    format: String,
    file: String,
    sha256: String,
    expected_units: Vec<Unit>,
    #[serde(default)]
    known_omissions: Vec<KnownOmission>,
    coverage: String,
    reasons: Vec<String>,
    limits: Value,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parser_build_sha(root: &std::path::Path) -> String {
    let mut sha = Sha256::new();
    for relative in ["src/main.rs", "src/readers.rs", "Cargo.toml", "Cargo.lock"] {
        sha.update(relative.as_bytes());
        sha.update(fs::read(root.join(relative)).unwrap_or_default());
    }
    hex::encode(sha.finalize())
}

fn native_pin(fmt: &str) -> Option<String> {
    // The explicit ZIP composite profile admits PDF leaves, so it pins PDFium
    // even when a given fixture happens to contain only non-PDF members.
    if !matches!(fmt, "pdf" | "zip") {
        return None;
    }
    let dir = std::env::var("PDFIUM_DYNAMIC_LIB_PATH").ok()?;
    let name = if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };
    fs::read(std::path::Path::new(&dir).join(name))
        .ok()
        .map(|bytes| digest(&bytes))
}

fn peak_rss_bytes() -> u64 {
    let mut r = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, r.as_mut_ptr()) } != 0 {
        return 0;
    }
    let v = unsafe { r.assume_init().ru_maxrss.max(0) as u64 };
    if cfg!(target_os = "macos") {
        v
    } else {
        v * 1024
    }
}

fn row_result(row: &Row, root: &std::path::Path, build: &str) -> Value {
    let t0 = Instant::now();
    let raw = fs::read(root.join(&row.file)).expect("verified fixed fixture");
    let raw_hash_ok = digest(&raw) == row.sha256;
    let pin = native_pin(&row.format);
    let pin_ok =
        !matches!(row.format.as_str(), "pdf" | "zip") || pin.as_deref() == Some(PDFIUM_SHA256);
    let extracted = if raw_hash_ok && pin_ok {
        inspect(&row.format, &raw, &row.limits)
    } else {
        readers::Extracted {
            units: vec![],
            coverage: "IntegrityFailure",
            reasons: vec![if !raw_hash_ok {
                "RawHashMismatch"
            } else {
                "NativePinMismatch"
            }],
            known_omissions: vec![],
        }
    };
    let reparsed = if raw_hash_ok && pin_ok {
        inspect(&row.format, &raw, &row.limits)
    } else {
        extracted.clone()
    };
    let mut locators = std::collections::BTreeSet::new();
    let mut roundtrip_failures = 0;
    for u in &extracted.units {
        let key = serde_json::to_string(&u.locator).unwrap();
        if !locators.insert(key) {
            roundtrip_failures += 1;
        }
        let found: Vec<&Unit> = reparsed
            .units
            .iter()
            .filter(|other| other.locator == u.locator)
            .collect();
        if found.len() != 1 || found[0].text != u.text || found[0].kind != u.kind {
            roundtrip_failures += 1;
        }
    }
    let matched = row
        .expected_units
        .iter()
        .filter(|x| extracted.units.contains(x))
        .count();
    let missed = row.expected_units.len() - matched;
    let unexpected = extracted
        .units
        .iter()
        .filter(|x| !row.expected_units.contains(x))
        .count();
    let reasons: Vec<&str> = extracted.reasons.clone();
    let matched_coverage = extracted.coverage == row.coverage
        && reasons == row.reasons.iter().map(String::as_str).collect::<Vec<_>>();
    let matched_omissions = extracted.known_omissions == row.known_omissions;
    let worker_result_bytes = serde_json::to_vec(&json!({
        "coverage": extracted.coverage,
        "reasons": extracted.reasons,
        "units": extracted.units,
        "known_omissions": extracted.known_omissions,
    }))
    .unwrap()
    .len();
    let wall_ms = t0.elapsed().as_millis();
    let peak_rss = peak_rss_bytes();
    let qualified = missed == 0
        && unexpected == 0
        && roundtrip_failures == 0
        && matched_coverage
        && matched_omissions
        && raw_hash_ok
        && pin_ok
        && wall_ms <= 10_000
        && peak_rss <= 2_147_483_648
        && worker_result_bytes <= 16_777_216;
    let mut result = json!({
        "format":row.format,
        "parser_build_sha256":build,
        "native_pin":pin,
        "fixture_id":row.id,
        "matched_units":matched,
        "missed_units":missed,
        "unexpected_units":unexpected,
        "locator_roundtrip_failures":roundtrip_failures,
        "coverage":extracted.coverage,
        "reason":reasons,
        "expected_coverage":row.coverage,
        "expected_reason":row.reasons,
        "known_omissions":extracted.known_omissions,
        "omissions_match":matched_omissions,
        "qualified":qualified,
        "wall_ms":wall_ms,
        "peak_rss_bytes":peak_rss,
        "scratch_bytes":0,
        "result_bytes":worker_result_bytes,
        "license_security":"not-executed",
    });
    if worker_result_bytes > 16_777_216 {
        result["qualified"] = json!(false);
    }
    result
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin()
        .take(16_777_216)
        .read_to_end(&mut bytes)
        .expect("bounded request");
    let request: Request = serde_json::from_slice(&bytes).expect("fixed corpus request");
    let build = parser_build_sha(&request.root);
    let mut failed = false;
    for row in &request.rows {
        let result = row_result(row, &request.root, &build);
        failed |= result["qualified"] != true;
        println!("{}", result);
    }
    if failed {
        std::process::exit(1);
    }
}
