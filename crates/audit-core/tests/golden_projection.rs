//! adapter_version discipline (design §7.2, §14.1): the projection output of
//! every accepted fixture is pinned per (source_format, adapter_version) in
//! `spec/telemetry/audit-adapter-golden.json`. Sections are append-only:
//!
//! - A changed projection needs a new LEGACY_ADAPTER_VERSION and a new
//!   section, never an edit of an existing entry.
//! - Entries are keyed `<fixture>@<first 16 hex of sha256(canonical input
//!   row JSON)>`, so adding, renaming or editing a fixture adds an entry
//!   instead of rewriting one; stale entries stay as history.
//! - Every section older than LEGACY_ADAPTER_VERSION is frozen by the sha256
//!   of its canonical JSON in [`FROZEN_SECTION_DIGESTS`], so editing or
//!   deleting it needs a second, visible change to this file.
//!
//! The digest is `sha256(AuditEnvelope::to_json_string())` (compact, keys in
//! byte order). It pins the Rust projection only; it is not the Store's
//! `kp-audit-jsonb-sha256-v1` envelope digest.

mod common;

use std::collections::BTreeMap;

use audit_core::chain::to_hex;
use audit_core::envelope::LEGACY_SOURCE_FORMAT;
use audit_core::{LEGACY_ADAPTER_VERSION, envelope_digest, project};
use common::{
    accepted_fixtures, check_golden, entry_key, fixture_named, is_entry_key, section_digest,
};
use serde_json::{Map, Value, json};

const GOLDEN: &str = include_str!("../../../spec/telemetry/audit-adapter-golden.json");
const BUMP: &str = "projection output changed: bump LEGACY_ADAPTER_VERSION and add a new version section; never edit an existing version's entries";

/// `(adapter version, sha256 hex of the section's canonical JSON)` of every
/// section older than [`LEGACY_ADAPTER_VERSION`]. Add the previous section's
/// digest here in the same change that bumps the version. Empty while only
/// version 1 exists.
const FROZEN_SECTION_DIGESTS: &[(i32, &str)] = &[];

fn computed() -> BTreeMap<String, String> {
    accepted_fixtures()
        .into_iter()
        .map(|fixture| {
            let envelope = project(&fixture.row)
                .unwrap_or_else(|r| panic!("{}: rejected with {r}", fixture.name));
            (
                entry_key(fixture.name, &fixture.row),
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
    if let Err(problems) = check_golden(
        &golden(),
        LEGACY_ADAPTER_VERSION,
        FROZEN_SECTION_DIGESTS,
        &computed(),
        BUMP,
    ) {
        panic!("{problems}");
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

#[test]
fn entries_are_keyed_by_the_input_row() {
    let fixture = fixture_named("document.moved");
    let key = entry_key(fixture.name, &fixture.row);
    assert!(is_entry_key(&key), "{key}");
    assert!(key.starts_with("document.moved@"));
    assert_eq!(key, entry_key(fixture.name, &fixture.row), "deterministic");
    let mut edited = fixture.row.clone();
    edited.reason_bytes = Some(6);
    assert_ne!(
        entry_key(fixture.name, &edited),
        key,
        "an edited input adds an entry instead of rewriting one"
    );
    assert!(computed().contains_key(&key));
}

fn synthetic(entries: &[(&str, &str)]) -> Value {
    Value::Object(
        entries
            .iter()
            .map(|(key, digest)| ((*key).to_owned(), json!(digest)))
            .collect(),
    )
}

#[test]
fn sections_older_than_the_current_version_are_frozen() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let old = synthetic(&[("fixture@0123456789abcdef", &a)]);
    let current: BTreeMap<String, String> = [("fixture@fedcba9876543210".to_owned(), b.clone())]
        .into_iter()
        .collect();
    let versions = |old: Option<Value>| {
        let mut versions = Map::new();
        if let Some(old) = old {
            versions.insert("1".to_owned(), old);
        }
        versions.insert(
            "2".to_owned(),
            synthetic(&[("fixture@fedcba9876543210", &b)]),
        );
        versions
    };
    let pinned = section_digest(&old);
    assert_eq!(
        check_golden(
            &versions(Some(old.clone())),
            2,
            &[(1, &pinned)],
            &current,
            BUMP
        ),
        Ok(())
    );
    let unfrozen = check_golden(&versions(Some(old.clone())), 2, &[], &current, BUMP)
        .expect_err("a section older than the current version needs a frozen digest");
    assert!(
        unfrozen.contains("no FROZEN_SECTION_DIGESTS entry"),
        "{unfrozen}"
    );
    let edited = synthetic(&[("fixture@0123456789abcdef", &b)]);
    let changed = check_golden(&versions(Some(edited)), 2, &[(1, &pinned)], &current, BUMP)
        .expect_err("editing a frozen section");
    assert!(changed.contains("changed after it was frozen"), "{changed}");
    let deleted = check_golden(&versions(None), 2, &[(1, &pinned)], &current, BUMP)
        .expect_err("deleting a frozen section");
    assert!(deleted.contains("frozen section 1 is missing"), "{deleted}");
    let newer = check_golden(&versions(Some(old)), 1, &[], &BTreeMap::new(), BUMP)
        .expect_err("a section newer than the adapter version");
    assert!(newer.contains("section 2 is not a version"), "{newer}");
}

#[test]
fn failure_messages_name_only_the_offending_entries() {
    let digest = |c: char| c.to_string().repeat(64);
    let computed: BTreeMap<String, String> = [
        ("kept@0000000000000001", digest('1')),
        ("changed@0000000000000002", digest('2')),
        ("added@0000000000000003", digest('3')),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let mut versions = Map::new();
    versions.insert(
        "1".to_owned(),
        synthetic(&[
            ("kept@0000000000000001", &digest('1')),
            ("changed@0000000000000002", &digest('9')),
            ("removed@0000000000000004", &digest('4')),
        ]),
    );
    let message = check_golden(&versions, 1, &[], &computed, BUMP).expect_err("drift");
    assert!(message.contains(&format!("{BUMP}: changed@0000000000000002")));
    assert!(message.contains(&format!("\"added@0000000000000003\": \"{}\"", digest('3'))));
    assert!(!message.contains("kept@"), "{message}");
    assert!(
        !message.contains(&digest('2')),
        "the new digest of a changed entry is never offered for re-blessing"
    );
    assert!(!message.contains("removed@"), "stale entries are history");
}
