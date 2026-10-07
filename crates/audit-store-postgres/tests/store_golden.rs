//! Store-side adapter_version discipline (design §7.2, §14.1): the Store's
//! `kp-audit-jsonb-sha256-v1` envelope digest of every accepted audit-core
//! fixture is pinned per (source_format, adapter_version) in
//! `tests/data/store-envelope-golden.json`.
//!
//! The file is append-only: a version section is never edited. A changed
//! projection (or a changed jsonb normalization) needs a new
//! `LEGACY_ADAPTER_VERSION`, a new registered_types row set and a new
//! section here. Forgetting the bump is not only a CI failure: re-delivering
//! an already stored event with a different envelope under the same
//! adapter_version is a conflict, never an overwrite (tested below).
//!
//! Entries are keyed `<fixture>@<first 16 hex of sha256(canonical input row
//! JSON)>` over the row the Store test actually projects (fixture event id
//! included), with the same rules as audit-core's projection pin
//! (`tests/common/mod.rs` `check_golden`): editing a fixture input adds an
//! entry that must be re-projected and appended (the test fails until it
//! is), stale entries stay as history, a changed digest under an existing
//! key needs a version bump, and older sections are frozen by
//! [`FROZEN_SECTION_DIGESTS`].
//!
//! Fixture event ids are derived from the fixture name (not its position),
//! so adding a fixture never changes another fixture's digest.

#[path = "../../audit-core/tests/common/mod.rs"]
mod common;
mod support;

use std::collections::BTreeMap;

use audit_core::chain::to_hex;
use audit_core::envelope::LEGACY_SOURCE_FORMAT;
use audit_core::{
    AuditEnvelope, AuditStore, DocumentStagingProjection, IngestOutcome, LEGACY_ADAPTER_VERSION,
    Origin, StoreError, project,
};
use common::{accepted_fixtures, check_golden, entry_key, is_entry_key};
use serde_json::{Map, Value};
use support::*;
use uuid::Uuid;

const GOLDEN: &str = include_str!("data/store-envelope-golden.json");
const BUMP: &str = "Store envelope digest changed: bump LEGACY_ADAPTER_VERSION, register the new version and add a new section; never edit an existing version's entries";

/// `(adapter version, sha256 hex of the section's canonical JSON)` of every
/// section older than `LEGACY_ADAPTER_VERSION`. Add the previous section's
/// digest here in the same change that bumps the version. Empty while only
/// version 1 exists.
const FROZEN_SECTION_DIGESTS: &[(i32, &str)] = &[];

/// A UUIDv7-shaped id derived from the fixture name.
fn fixture_event_id(name: &str) -> Uuid {
    let digest = audit_core::envelope_digest(&format!("kp-audit-store-golden-v1:{name}"));
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn fixtures() -> Vec<(&'static str, DocumentStagingProjection)> {
    accepted_fixtures()
        .into_iter()
        .map(|fixture| {
            let mut row = fixture.row;
            row.event_id = fixture_event_id(fixture.name).to_string();
            (fixture.name, row)
        })
        .collect()
}

fn golden() -> Map<String, Value> {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden file is JSON");
    assert_eq!(golden["algorithm"], "kp-audit-jsonb-sha256-v1");
    assert_eq!(golden["source_format"], LEGACY_SOURCE_FORMAT);
    golden["versions"]
        .as_object()
        .cloned()
        .expect("versions object")
}

#[tokio::test]
async fn store_envelope_digests_are_pinned_for_the_current_adapter_version() {
    let versions = golden();
    let db = TestDb::start().await;
    let store = db.service_relay().await.store().await;
    let mut computed = BTreeMap::new();
    for (name, row) in fixtures() {
        let envelope = project(&row).unwrap_or_else(|r| panic!("{name}: rejected with {r}"));
        let receipt = store
            .ingest(&envelope)
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(receipt.outcome, IngestOutcome::Stored, "{name}");
        assert_eq!(receipt.adapter_version, LEGACY_ADAPTER_VERSION, "{name}");
        computed.insert(entry_key(name, &row), to_hex(&receipt.envelope_digest));
    }
    // The Store's digests are audit-core's (cross-checked per row).
    db.assert_store_conforms().await;
    if let Err(problems) = check_golden(
        &versions,
        LEGACY_ADAPTER_VERSION,
        FROZEN_SECTION_DIGESTS,
        &computed,
        BUMP,
    ) {
        panic!("{problems}");
    }
}

#[test]
fn entries_are_keyed_by_the_projected_input_row() {
    let fixtures = fixtures();
    let (name, row) = fixtures.first().expect("a fixture");
    let key = entry_key(name, row);
    assert!(
        is_entry_key(&key) && key.starts_with(&format!("{name}@")),
        "{key}"
    );
    // The Store pins the row it projects: the derived event id is part of
    // the input, so the Store key differs from audit-core's for the same
    // fixture, and any edited input column yields a new key.
    let original = accepted_fixtures()
        .into_iter()
        .find(|f| f.name == *name)
        .expect("same fixture");
    assert_ne!(entry_key(name, &original.row), key);
    let mut edited = row.clone();
    edited.occurred_at = "2026-10-08T00:00:00.000000Z".to_owned();
    assert_ne!(entry_key(name, &edited), key);
    // Every current key is pinned in the current section.
    let pinned = golden()
        .get(&LEGACY_ADAPTER_VERSION.to_string())
        .and_then(Value::as_object)
        .cloned()
        .expect("current section");
    for (name, row) in &fixtures {
        assert!(pinned.contains_key(&entry_key(name, row)), "{name}");
    }
}

#[test]
fn fixture_event_ids_are_distinct_and_independent_of_order() {
    let ids: Vec<Uuid> = fixtures()
        .iter()
        .map(|(_, row)| Uuid::parse_str(&row.event_id).expect("uuid"))
        .collect();
    let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(distinct.len(), ids.len());
    assert!(ids.iter().all(|id| id.get_version_num() == 7));
    assert_eq!(
        fixture_event_id("document.created"),
        fixture_event_id("document.created")
    );
}

#[tokio::test]
async fn a_forgotten_bump_is_a_conflict_not_an_overwrite() {
    let db = TestDb::start().await;
    let store = db.service_relay().await.store().await;
    let (name, row) = fixtures().into_iter().next().expect("at least one fixture");
    let stored = store
        .ingest(&project(&row).expect("projects"))
        .await
        .expect("stored");
    // A different envelope for the same event under the same
    // adapter_version, as a changed projection would produce (here one
    // projected provenance field differs).
    let mut changed = project(&row).expect("projects").into_value();
    let commitment = &mut changed["data"]["provenance"]["source_commitment"];
    assert!(
        commitment.is_string(),
        "{name}: projected source_commitment"
    );
    *commitment = Value::String("ee".repeat(32));
    let changed = AuditEnvelope::from_value(changed, Origin::Relay).expect("valid envelope");
    assert_eq!(changed.id(), fixture_event_id(name));
    assert_eq!(
        changed.as_value()["data"]["provenance"]["adapter_version"],
        LEGACY_ADAPTER_VERSION
    );
    assert!(matches!(
        store.ingest(&changed).await,
        Err(StoreError::Conflict { .. })
    ));
    let (head_seq, _, _) = head(&db.admin).await;
    let conflicts = control_events(&db.admin, "audit.integrity.conflict_detected").await;
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].1["event_id"],
        Value::String(fixture_event_id(name).to_string())
    );
    assert_eq!(conflicts[0].0, head_seq);
    // The stored row is untouched; the unchanged envelope is a duplicate.
    let again = store
        .ingest(&project(&row).expect("projects"))
        .await
        .expect("duplicate");
    assert_eq!(again.outcome, IngestOutcome::Duplicate);
    assert_eq!(again.seq, stored.seq);
    assert_eq!(again.envelope_digest, stored.envelope_digest);
    db.assert_store_conforms().await;
}
