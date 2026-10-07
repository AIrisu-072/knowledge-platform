//! The Store port's content-free types agree with the catalog: relay control
//! details validate as `relay_control` envelopes, and probe expectations are
//! the catalog's registered types.

mod common;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::port::precheck_ingest;
use audit_core::{
    AuditEnvelope, BoundedCode, Catalog, IngestRow, LEGACY_ADAPTER_VERSION, Origin, OutageCode,
    ProbeExpectation, ReceiptIdentity, ReconcileCounts, ReconcileMode, RelayControl,
    RelayControlKind, SourceMismatchCode, StoreError, validate_envelope,
};
use common::*;
use serde_json::{Value, json};
use uuid::Uuid;

fn relay_controls() -> Vec<RelayControl> {
    let event_id = Uuid::parse_str(EVENT_ID).expect("uuid");
    vec![
        RelayControl::from(RelayControlKind::ReplayRequested {
            event_id,
            previous_code: BoundedCode::new("delivery_unknown_at_limit").expect("bounded"),
        }),
        RelayControl::from(RelayControlKind::ReconciliationCompleted {
            run_id: Uuid::parse_str(OP).expect("uuid"),
            mode: ReconcileMode::Repair,
            watermark: 42,
            id_set_digest: [7; 32],
            counts: ReconcileCounts {
                ok: 40,
                delivered_missing: 1,
                replay_record_lost: 1,
                repaired_delivered_missing: 1,
                ..ReconcileCounts::default()
            },
        }),
        RelayControl::from(RelayControlKind::SourceMismatchDetected {
            event_id,
            code: SourceMismatchCode::SourceDigestMismatch,
        }),
        RelayControl::from(RelayControlKind::SourceMismatchDetected {
            event_id,
            code: SourceMismatchCode::ActorMismatch,
        }),
    ]
}

/// The envelope the Store builds from `RelayControl::details`.
fn envelope_for(control: &RelayControl) -> Value {
    let spec = Catalog::embedded()
        .get(control.event_type())
        .expect("relay control type is in the catalog");
    let mut value = control_envelope(spec, false);
    let mut details = control.details();
    details.insert("session_role".to_owned(), json!("audit_relay_operator_1"));
    value["data"]["details"] = Value::Object(details);
    value
}

#[test]
fn relay_control_details_validate_against_the_catalog() {
    let mut covered = std::collections::BTreeSet::new();
    for control in relay_controls() {
        let spec = Catalog::embedded()
            .get(control.event_type())
            .expect("catalog entry");
        assert_eq!(
            spec.origin,
            Origin::RelayControl,
            "{}",
            control.event_type()
        );
        let value = envelope_for(&control);
        validate_envelope(&value, Origin::RelayControl)
            .unwrap_or_else(|r| panic!("{}: {r}", control.event_type()));
        // Every catalog field is produced: the port carries the whole shape.
        let produced = value["data"]["details"].as_object().expect("details");
        for (name, _) in spec.detail_fields() {
            assert!(
                produced.contains_key(name),
                "{}: {name} is not produced by RelayControl",
                control.event_type()
            );
        }
        covered.insert(control.event_type());
    }
    let relay_control_types: std::collections::BTreeSet<&str> = Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin == Origin::RelayControl)
        .map(|spec| spec.event_type.as_str())
        .collect();
    assert_eq!(covered, relay_control_types);
}

#[test]
fn oversized_reconcile_counts_are_refused_by_the_catalog() {
    let control = RelayControl::from(RelayControlKind::ReconciliationCompleted {
        run_id: Uuid::parse_str(OP).expect("uuid"),
        mode: ReconcileMode::ReadOnly,
        watermark: 1,
        id_set_digest: [0; 32],
        counts: ReconcileCounts {
            ok: u64::MAX,
            ..ReconcileCounts::default()
        },
    });
    assert_eq!(
        validate_envelope(&envelope_for(&control), Origin::RelayControl),
        Err(at(audit_core::RejectionCode::InvalidField, "count_ok"))
    );
}

#[test]
fn probe_expectation_lists_the_registered_types_of_a_source() {
    let last_ack = ReceiptIdentity {
        seq: 9,
        event_id: Uuid::parse_str(EVENT_ID).expect("uuid"),
        envelope_digest: [3; 32],
    };
    let expectation =
        ProbeExpectation::from_catalog(Catalog::embedded(), DOCUMENT_SOURCE, Some(last_ack))
            .expect("document source has relay types");
    assert_eq!(expectation.source, DOCUMENT_SOURCE);
    assert_eq!(expectation.adapter_version, LEGACY_ADAPTER_VERSION);
    assert_eq!(expectation.types.len(), 21);
    assert!(expectation.types.iter().any(|t| t == "folder.moved"));
    assert_eq!(expectation.last_ack, Some(last_ack));
    assert!(
        ProbeExpectation::from_catalog(
            Catalog::embedded(),
            audit_core::catalog::AUDIT_STORE_SOURCE,
            None
        )
        .is_none(),
        "control sources are never ingested"
    );
}

fn ingest_row(status: &str, code: Option<&str>) -> IngestRow {
    IngestRow {
        status: status.to_owned(),
        seq: None,
        envelope_digest: None,
        adapter_version: None,
        code: code.map(str::to_owned),
    }
}

#[test]
fn terminal_errors_are_only_decoded_from_store_verdict_rows() {
    // Outside the crate a terminal error can only come from a decoded row
    // (constructing `StoreError::Conflict { .. }` directly does not compile;
    // see the compile_fail example on `StoreError`).
    let conflict = ingest_row("conflict", None)
        .into_result()
        .expect_err("verdict");
    assert!(matches!(conflict, StoreError::Conflict { .. }));
    assert!(conflict.is_terminal());
    let rejected = ingest_row("rejected", Some("invalid_envelope"))
        .into_result()
        .expect_err("verdict");
    match &rejected {
        StoreError::Rejected { code, .. } => assert_eq!(code.as_str(), "invalid_envelope"),
        other => panic!("expected a rejection, got {other:?}"),
    }
    // A SQL failure is classified, never a verdict.
    let outage = StoreError::outage(audit_core::classify_sqlstate("23505"));
    assert!(outage.is_outage());
    assert_eq!(outage.outage_code(), Some(OutageCode::Other));
}

#[test]
fn ingest_refuses_non_relay_envelopes_without_a_round_trip() {
    let relay = audit_core::project(&fixture_named("document.created").row).expect("projects");
    assert_eq!(relay.origin(), Origin::Relay);
    assert_eq!(precheck_ingest(&relay), Ok(()));
    for (name, origin, value) in control_envelopes() {
        let envelope =
            AuditEnvelope::from_value(value, origin).unwrap_or_else(|r| panic!("{name}: {r}"));
        assert_eq!(envelope.origin(), origin, "{name}: the validated path");
        let error = precheck_ingest(&envelope).expect_err("control envelopes are not ingested");
        assert!(error.is_terminal(), "{name}");
        match error {
            StoreError::Rejected { code, .. } => {
                assert_eq!(code.as_str(), "control_type_forbidden", "{name}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}
