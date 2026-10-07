//! adapter_version discipline (design §7.2, §14.1): the projection output of
//! every accepted fixture is pinned per (source_format, adapter_version) in
//! `spec/telemetry/audit-adapter-golden.json`. Version sections are
//! append-only: a changed projection needs a new LEGACY_ADAPTER_VERSION and a
//! new section, never an edit of an existing one.
//!
//! The digest is `sha256(AuditEnvelope::to_json_string())` (compact, keys in
//! byte order). It pins the Rust projection only; it is not the Store's
//! `kp-audit-jsonb-sha256-v1` envelope digest.

mod common;

use std::collections::BTreeMap;

use audit_core::chain::to_hex;
use audit_core::envelope::LEGACY_SOURCE_FORMAT;
use audit_core::{LEGACY_ADAPTER_VERSION, envelope_digest, project};
use common::accepted_fixtures;
use serde_json::{Map, Value};

const GOLDEN: &str = include_str!("../../../spec/telemetry/audit-adapter-golden.json");
const BUMP: &str = "projection output changed: bump LEGACY_ADAPTER_VERSION and add a new version section; never edit an existing version's entries";

fn computed() -> BTreeMap<String, String> {
    accepted_fixtures()
        .into_iter()
        .map(|fixture| {
            let envelope = project(&fixture.row)
                .unwrap_or_else(|r| panic!("{}: rejected with {r}", fixture.name));
            (
                fixture.name.to_owned(),
                to_hex(&envelope_digest(&envelope.to_json_string())),
            )
        })
        .collect()
}

fn golden() -> Map<String, Value> {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden file is JSON");
    assert_eq!(golden["algorithm"], "rust-compact-sorted-sha256");
    assert_eq!(golden["source_format"], LEGACY_SOURCE_FORMAT);
    golden["versions"]
        .as_object()
        .cloned()
        .expect("versions object")
}

#[test]
fn projection_output_is_pinned_for_the_current_adapter_version() {
    let versions = golden();
    for (key, section) in &versions {
        let version: i32 = key.parse().expect("version keys are integers");
        assert!(
            (1..=LEGACY_ADAPTER_VERSION).contains(&version),
            "{BUMP}: section {key} is newer than LEGACY_ADAPTER_VERSION {LEGACY_ADAPTER_VERSION}"
        );
        let entries = section.as_object().expect("section is an object");
        assert!(!entries.is_empty(), "section {key} is empty");
        for (name, digest) in entries {
            assert!(
                digest
                    .as_str()
                    .is_some_and(audit_core::kinds::is_hex_digest),
                "section {key}: {name} is not a sha256 hex digest"
            );
        }
    }
    let computed = computed();
    let rendered = serde_json::to_string_pretty(&computed).expect("render");
    let Some(current) = versions.get(&LEGACY_ADAPTER_VERSION.to_string()) else {
        panic!("{BUMP}: no section for version {LEGACY_ADAPTER_VERSION}; computed:\n{rendered}");
    };
    let pinned: BTreeMap<String, String> = current
        .as_object()
        .expect("section")
        .iter()
        .map(|(name, digest)| (name.clone(), digest.as_str().unwrap_or("").to_owned()))
        .collect();
    assert_eq!(
        pinned.keys().collect::<Vec<_>>(),
        computed.keys().collect::<Vec<_>>(),
        "{BUMP}: the current section must pin every accepted fixture exactly once; computed:\n{rendered}"
    );
    for (name, digest) in &computed {
        assert_eq!(
            pinned.get(name),
            Some(digest),
            "{BUMP}: {name}; computed:\n{rendered}"
        );
    }
}

#[test]
fn golden_digests_are_distinct_per_fixture() {
    let computed = computed();
    let mut seen = std::collections::BTreeSet::new();
    for (name, digest) in &computed {
        assert!(
            seen.insert(digest),
            "{name} duplicates another fixture's output"
        );
    }
}
