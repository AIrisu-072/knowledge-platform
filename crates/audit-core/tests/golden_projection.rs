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
use audit_core::kinds::is_hex_digest;
use audit_core::{DocumentStagingProjection, LEGACY_ADAPTER_VERSION, envelope_digest, project};
use common::{accepted_fixtures, fixture_named};
use serde_json::{Map, Value, json};

const GOLDEN: &str = include_str!("../../../spec/telemetry/audit-adapter-golden.json");
const BUMP: &str = "projection output changed: bump LEGACY_ADAPTER_VERSION and add a new version section; never edit an existing version's entries";

/// `(adapter version, sha256 hex of the section's canonical JSON)` of every
/// section older than [`LEGACY_ADAPTER_VERSION`]. Add the previous section's
/// digest here in the same change that bumps the version. Empty while only
/// version 1 exists.
const FROZEN_SECTION_DIGESTS: &[(i32, &str)] = &[];

/// Compact JSON with object keys in byte order at every depth.
fn canonical_text(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        canonical_text(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_text).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

/// Every column of the claim row. The exhaustive destructuring makes a new
/// column a compile error here, so the key always covers the whole input.
fn row_json(row: &DocumentStagingProjection) -> Value {
    let DocumentStagingProjection {
        event_id,
        event_type,
        source,
        subject,
        actor_identity_provider,
        actor_principal_id,
        resource_type,
        resource_id,
        resource_version_id,
        result,
        trace_id,
        occurred_at,
        oversize,
        data,
        data_kind,
        reason_kind,
        reason_bytes,
        source_intact,
        source_commitment,
        registration_kind,
    } = row;
    json!({
        "event_id": event_id,
        "event_type": event_type,
        "source": source,
        "subject": subject,
        "actor_identity_provider": actor_identity_provider,
        "actor_principal_id": actor_principal_id,
        "resource_type": resource_type,
        "resource_id": resource_id,
        "resource_version_id": resource_version_id,
        "result": result,
        "trace_id": trace_id,
        "occurred_at": occurred_at,
        "oversize": oversize,
        "data": data,
        "data_kind": data_kind,
        "reason_kind": reason_kind,
        "reason_bytes": reason_bytes,
        "source_intact": source_intact,
        "source_commitment": source_commitment,
        "registration_kind": registration_kind,
    })
}

fn entry_key(name: &str, row: &DocumentStagingProjection) -> String {
    let digest = to_hex(&envelope_digest(&canonical_text(&row_json(row))));
    format!("{name}@{}", &digest[..16])
}

fn is_entry_key(key: &str) -> bool {
    key.rsplit_once('@').is_some_and(|(name, hash)| {
        !name.is_empty()
            && hash.len() == 16
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn section_digest(section: &Value) -> String {
    to_hex(&envelope_digest(&canonical_text(section)))
}

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

/// Checks the golden sections against the computed projection. Failure
/// messages name only the offending entries: new keys with their digest (to
/// append), changed keys without it (a version bump is required).
fn check_golden(
    versions: &Map<String, Value>,
    current: i32,
    frozen: &[(i32, &str)],
    computed: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    for (key, section) in versions {
        let Some(version) = key
            .parse::<i32>()
            .ok()
            .filter(|v| (1..=current).contains(v))
        else {
            problems.push(format!(
                "{BUMP}: section {key} is not a version in 1..={current}"
            ));
            continue;
        };
        let Some(entries) = section.as_object().filter(|e| !e.is_empty()) else {
            problems.push(format!("section {key} must be a non-empty object"));
            continue;
        };
        for (name, digest) in entries {
            if !is_entry_key(name) || !digest.as_str().is_some_and(is_hex_digest) {
                problems.push(format!(
                    "section {key}: entry {name} must be <fixture>@<16 hex> -> sha256 hex"
                ));
            }
        }
        if version < current {
            match frozen.iter().find(|(v, _)| *v == version) {
                None => problems.push(format!(
                    "section {key} is older than {current} but has no FROZEN_SECTION_DIGESTS entry"
                )),
                Some((_, pinned)) if *pinned != section_digest(section) => problems.push(format!(
                    "section {key} changed after it was frozen; never edit an existing version's entries"
                )),
                Some(_) => {}
            }
        }
    }
    for (version, _) in frozen {
        if *version >= current || !versions.contains_key(&version.to_string()) {
            problems.push(format!(
                "frozen section {version} is missing or not older than {current}"
            ));
        }
    }
    match versions
        .get(&current.to_string())
        .and_then(Value::as_object)
    {
        None => problems.push(format!("{BUMP}: no section for version {current}")),
        Some(pinned) => {
            for (key, digest) in computed {
                match pinned.get(key).and_then(Value::as_str) {
                    None => problems.push(format!(
                        "new fixture input, append to section {current}: \"{key}\": \"{digest}\""
                    )),
                    Some(pinned) if pinned != digest => {
                        problems.push(format!("{BUMP}: {key}"));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn projection_output_is_pinned_for_the_current_adapter_version() {
    if let Err(problems) = check_golden(
        &golden(),
        LEGACY_ADAPTER_VERSION,
        FROZEN_SECTION_DIGESTS,
        &computed(),
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
        check_golden(&versions(Some(old.clone())), 2, &[(1, &pinned)], &current),
        Ok(())
    );
    let unfrozen = check_golden(&versions(Some(old.clone())), 2, &[], &current)
        .expect_err("a section older than the current version needs a frozen digest");
    assert!(
        unfrozen.contains("no FROZEN_SECTION_DIGESTS entry"),
        "{unfrozen}"
    );
    let edited = synthetic(&[("fixture@0123456789abcdef", &b)]);
    let changed = check_golden(&versions(Some(edited)), 2, &[(1, &pinned)], &current)
        .expect_err("editing a frozen section");
    assert!(changed.contains("changed after it was frozen"), "{changed}");
    let deleted = check_golden(&versions(None), 2, &[(1, &pinned)], &current)
        .expect_err("deleting a frozen section");
    assert!(deleted.contains("frozen section 1 is missing"), "{deleted}");
    let newer = check_golden(&versions(Some(old)), 1, &[], &BTreeMap::new())
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
    let message = check_golden(&versions, 1, &[], &computed).expect_err("drift");
    assert!(message.contains(&format!("{BUMP}: changed@0000000000000002")));
    assert!(message.contains(&format!("\"added@0000000000000003\": \"{}\"", digest('3'))));
    assert!(!message.contains("kept@"), "{message}");
    assert!(
        !message.contains(&digest('2')),
        "the new digest of a changed entry is never offered for re-blessing"
    );
    assert!(!message.contains("removed@"), "stale entries are history");
}
