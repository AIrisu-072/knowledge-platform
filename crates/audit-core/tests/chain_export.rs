//! Hash-chain vectors (cross-checked by an independent implementation) and
//! out-of-database export verification.

use audit_core::catalog::{AUDIT_STORE_SOURCE, DOCUMENT_SOURCE};
use audit_core::chain::{GENESIS_PREIMAGE, parse_hex32, to_hex};
use audit_core::{
    Anchor, ChainVerdict, Checkpoint, CheckpointComparison as Cmp, EpochTransition, ExportError,
    GENESIS, RecoveryRecord, assess_recovery, chain_next, compare_checkpoint, envelope_digest,
    expired_set_digest, verify_export, verify_export_subset, verify_identity_chain,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const GENESIS_HEX: &str = "9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344";
const VECTOR_EVENT_1: &str = "0199a1b2-0000-7000-8000-000000000001";
const VECTOR_EVENT_2: &str = "0199a1b2-0000-7000-8000-000000000002";
const VECTOR_BODY_1: &str = r#"{"example":"envelope"}"#;
const VECTOR_DIGEST_1: &str = "c147ac99fc21ba6cc69a47812b1bb2415e353995e22b65d6a2b58a5222dd5f6f";
const VECTOR_CHAIN_1: &str = "babbd336ec2c8556082424a253afb6d8e88ef5a63c9cbcab6cea8f441ad528e1";
const VECTOR_CHAIN_2: &str = "1496cf0e490216c2ca995db8ac0a340562a691cf1d2f955865624900b4856106";
const EXPIRED_SET_3_5_9: &str = "66efae7170a5138bff29bb1c633cd75e5eb7345491ca9c354aea98c0e08a45e6";

fn uuid(text: &str) -> Uuid {
    Uuid::parse_str(text).expect("uuid")
}

#[test]
fn genesis_is_the_sha256_of_its_preimage() {
    let computed: [u8; 32] = Sha256::digest(GENESIS_PREIMAGE).into();
    assert_eq!(GENESIS, computed);
    assert_eq!(to_hex(&GENESIS), GENESIS_HEX);
}

#[test]
fn chain_vectors_match_the_documented_values() {
    let digest = envelope_digest(VECTOR_BODY_1);
    assert_eq!(to_hex(&digest), VECTOR_DIGEST_1);
    let first = chain_next(&GENESIS, 1, uuid(VECTOR_EVENT_1), &digest);
    assert_eq!(to_hex(&first), VECTOR_CHAIN_1);
    let second = chain_next(&first, 2, uuid(VECTOR_EVENT_2), &[0x11; 32]);
    assert_eq!(to_hex(&second), VECTOR_CHAIN_2);
}

#[test]
fn expired_set_digest_vectors_match_the_documented_values() {
    let empty: [u8; 32] = Sha256::digest(b"kp-audit-expired-set-v1").into();
    assert_eq!(expired_set_digest(&[]), empty);
    assert_eq!(to_hex(&expired_set_digest(&[3, 5, 9])), EXPIRED_SET_3_5_9);
    assert_eq!(
        expired_set_digest(&[9, 3, 5]),
        expired_set_digest(&[3, 5, 9])
    );
    let mut preimage = b"kp-audit-expired-set-v1".to_vec();
    for seq in [3_i64, 5, 9] {
        preimage.extend_from_slice(&seq.to_be_bytes());
    }
    let direct: [u8; 32] = Sha256::digest(&preimage).into();
    assert_eq!(expired_set_digest(&[3, 5, 9]), direct);
}

#[test]
fn hex_helpers_are_strict() {
    assert_eq!(parse_hex32(GENESIS_HEX), Some(GENESIS));
    assert_eq!(parse_hex32(&GENESIS_HEX.to_uppercase()), None);
    assert_eq!(parse_hex32(&GENESIS_HEX[..62]), None);
}

struct Spec {
    event: Uuid,
    origin: &'static str,
    body: Option<String>,
    digest: [u8; 32],
    expired_by: Option<i64>,
    epoch: i64,
}

/// PostgreSQL `jsonb::text`-style rendering (", " and ": " separators).
fn pg_text(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let members: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", Value::String(k.clone()), pg_text(v)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(pg_text).collect();
            format!("[{}]", items.join(", "))
        }
        other => other.to_string(),
    }
}

fn event_id(n: u128) -> Uuid {
    Uuid::from_u128(0x0199a1b2_0000_7000_8000_000000000000 | n)
}

fn body_spec(event: Uuid, origin: &'static str, body: &Value) -> Spec {
    let body = pg_text(body);
    Spec {
        event,
        origin,
        digest: envelope_digest(&body),
        body: Some(body),
        expired_by: None,
        epoch: 1,
    }
}

/// A relay-origin `document.created`-like body.
fn spec(n: u128) -> Spec {
    let event = event_id(n);
    body_spec(
        event,
        "relay",
        &json!({
            "id": event.to_string(),
            "data": {"n": u64::try_from(n & 0xffff).expect("small")},
            "type": "document.created",
            "source": DOCUMENT_SOURCE
        }),
    )
}

/// An origin=store control body with the given type and details.
fn control(n: u128, event_type: &str, details: Value) -> Spec {
    let event = event_id(0xc000 | n);
    body_spec(
        event,
        "store",
        &json!({
            "id": event.to_string(),
            "data": {"details": details},
            "type": event_type,
            "source": AUDIT_STORE_SOURCE
        }),
    )
}

fn expire(mut s: Spec, by: i64) -> Spec {
    s.body = None;
    s.expired_by = Some(by);
    s
}

struct Built {
    lines: Vec<String>,
    chains: Vec<[u8; 32]>,
}

fn build(specs: &[Spec], start_seq: i64, start_chain: [u8; 32]) -> Built {
    let mut prev = start_chain;
    let mut lines = Vec::new();
    let mut chains = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        let seq = start_seq + 1 + i64::try_from(i).expect("small");
        let chain = chain_next(&prev, seq, s.event, &s.digest);
        lines.push(line(seq, s, &prev, &chain));
        chains.push(chain);
        prev = chain;
    }
    Built { lines, chains }
}

fn line(seq: i64, s: &Spec, prev: &[u8; 32], chain: &[u8; 32]) -> String {
    format!(
        r#"{{"seq":{seq},"event_id":"{}","origin":"{}","envelope_digest":"{}","prev_chain":"{}","chain":"{}","recovery_epoch":{},"expired":{},"expired_by_seq":{},"envelope":{}}}"#,
        s.event,
        s.origin,
        to_hex(&s.digest),
        to_hex(prev),
        to_hex(chain),
        s.epoch,
        s.expired_by.is_some(),
        s.expired_by.map_or("null".to_owned(), |by| by.to_string()),
        s.body.as_deref().unwrap_or("null"),
    )
}

fn text(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn five() -> (Vec<Spec>, Built) {
    let specs: Vec<Spec> = (1..=5).map(spec).collect();
    let built = build(&specs, 0, GENESIS);
    (specs, built)
}

#[test]
fn contiguous_export_from_genesis_verifies() {
    let (_, built) = five();
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("valid export");
    assert!(report.anchored);
    assert_eq!(report.rows, 5);
    assert_eq!(report.bodies, 5);
    assert_eq!(report.expired, 0);
    assert_eq!(report.head.seq, 5);
    assert_eq!(report.head.chain, built.chains[4]);
    assert_eq!(report.head.epoch, 1);
    assert_eq!(
        report.anchor,
        Some(Checkpoint {
            epoch: 1,
            seq: 0,
            chain: GENESIS
        })
    );
    // A missing final newline is fine.
    assert!(verify_export(&built.lines.join("\n"), Anchor::Genesis).is_ok());
}

#[test]
fn empty_export_reports_the_anchor_as_head() {
    let report = verify_export("", Anchor::Genesis).expect("empty is consistent");
    assert_eq!(report.rows, 0);
    assert_eq!(report.head.seq, 0);
    assert_eq!(report.head.chain, GENESIS);
}

#[test]
fn tampered_body_is_detected_by_digest() {
    let (_, mut built) = five();
    built.lines[2] = built.lines[2].replace(r#"{"n": 3}"#, r#"{"n": 4}"#);
    assert!(built.lines[2].contains(r#"{"n": 4}"#), "fixture edited");
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::DigestMismatch { line: 3 })
    );
}

#[test]
fn whitespace_changes_in_the_body_are_tampering() {
    let (_, mut built) = five();
    built.lines[0] = built.lines[0].replace(r#""data": {"#, r#""data":{"#);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::DigestMismatch { line: 1 })
    );
}

#[test]
fn rewritten_digest_column_breaks_the_chain() {
    let (mut specs, honest) = five();
    let forged = format!(
        "{{\"id\": \"{}\", \"forged\": true, \"type\": \"document.created\", \"source\": \"{DOCUMENT_SOURCE}\"}}",
        specs[1].event
    );
    specs[1].digest = envelope_digest(&forged);
    specs[1].body = Some(forged);
    let mut lines = honest.lines.clone();
    // Body and digest rewritten consistently, chain left untouched.
    lines[1] = line(2, &specs[1], &honest.chains[0], &honest.chains[1]);
    assert_eq!(
        verify_export(&text(&lines), Anchor::Genesis),
        Err(ExportError::ChainMismatch { line: 2 })
    );
}

#[test]
fn deleted_line_is_detected() {
    let (_, mut built) = five();
    built.lines.remove(2);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::SeqGap {
            line: 3,
            expected: 3,
            found: 4
        })
    );
}

#[test]
fn reordered_lines_are_detected() {
    let (_, mut built) = five();
    built.lines.swap(1, 2);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::SeqGap {
            line: 2,
            expected: 2,
            found: 3
        })
    );
}

#[test]
fn recomputed_chain_with_renumbered_seq_breaks_linkage() {
    // Delete row 3 and renumber/re-chain the rest: the result no longer links
    // to the trusted checkpoint taken at seq 5.
    let (specs, built) = five();
    let kept: Vec<Spec> = specs
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, s)| s)
        .collect();
    let forged = build(&kept, 0, GENESIS);
    let report =
        verify_export(&text(&forged.lines), Anchor::Genesis).expect("self-consistent forgery");
    let trusted = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: built.chains[4],
    };
    assert_eq!(compare_checkpoint(&report, &trusted), Cmp::StoreBehind);
    let trusted_at_4 = Checkpoint {
        epoch: 1,
        seq: 4,
        chain: built.chains[3],
    };
    assert_eq!(compare_checkpoint(&report, &trusted_at_4), Cmp::Mismatch);
}

#[test]
fn expired_rows_keep_the_chain_without_a_body() {
    // Evidence past the exported range is reported, not assumed.
    let (mut specs, _) = five();
    specs[1] = expire(spec(2), 99);
    let built = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("expired row is fine");
    assert_eq!(report.expired, 1);
    assert_eq!(report.bodies, 4);
    assert_eq!(report.unverified_expiry_evidence, 1);

    let (mut specs, _) = five();
    specs[1].expired_by = Some(99);
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::BodyState { line: 2 })
    );

    let (mut specs, _) = five();
    specs[1].body = None;
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::BodyState { line: 2 })
    );
}

#[test]
fn expired_flag_and_expired_by_seq_must_agree() {
    let (mut specs, _) = five();
    specs[1] = expire(spec(2), 99);
    let built = build(&specs, 0, GENESIS);
    let no_evidence = built.lines[1].replace("\"expired_by_seq\":99", "\"expired_by_seq\":null");
    let unexpired = built.lines[0].replace("\"expired_by_seq\":null", "\"expired_by_seq\":7");
    let missing_key = built.lines[0].replace(",\"expired_by_seq\":null", "");
    for (input, line) in [
        (format!("{}\n{no_evidence}", built.lines[0]), 2),
        (unexpired, 1),
        (missing_key, 1),
    ] {
        assert_eq!(
            verify_export(&input, Anchor::Genesis),
            Err(ExportError::Malformed { line }),
            "{input}"
        );
    }
}

fn retention(n: u128, count: i64, first: Option<i64>, last: Option<i64>, set: &[i64]) -> Spec {
    control(
        n,
        "audit.retention.expired",
        json!({
            "count": count,
            "first_seq": first,
            "last_seq": last,
            "expired_set_digest": to_hex(&expired_set_digest(set)),
            "policy_id": "default"
        }),
    )
}

/// Rows 1..=5 (relay) with 2 and 4 expired by the retention event at 6.
fn retention_history(evidence: Spec) -> Vec<Spec> {
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[1] = expire(spec(2), 6);
    specs[3] = expire(spec(4), 6);
    specs.push(evidence);
    specs
}

#[test]
fn retention_evidence_inside_the_export_is_verified() {
    let specs = retention_history(retention(1, 2, Some(2), Some(4), &[2, 4]));
    let built = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("verified");
    assert_eq!(report.expired, 2);
    assert_eq!(report.unverified_expiry_evidence, 0);

    for (label, evidence) in [
        ("digest", retention(1, 2, Some(2), Some(4), &[2, 3])),
        ("count", retention(1, 3, Some(2), Some(4), &[2, 4])),
        ("first", retention(1, 2, Some(1), Some(4), &[2, 4])),
        ("last", retention(1, 2, Some(2), None, &[2, 4])),
        (
            "missing digest",
            control(
                1,
                "audit.retention.expired",
                json!({"count": 2, "first_seq": 2, "last_seq": 4}),
            ),
        ),
    ] {
        let built = build(&retention_history(evidence), 0, GENESIS);
        assert_eq!(
            verify_export(&text(&built.lines), Anchor::Genesis),
            Err(ExportError::ExpiryEvidenceMismatch { line: 6 }),
            "{label}"
        );
    }
}

#[test]
fn unreferenced_retention_evidence_must_match_an_empty_set() {
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs.push(retention(1, 0, None, None, &[]));
    let built = build(&specs, 0, GENESIS);
    assert!(verify_export(&text(&built.lines), Anchor::Genesis).is_ok());
    // Evidence claims a row the export shows with its body intact.
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs.push(retention(1, 1, Some(3), Some(3), &[3]));
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::ExpiryEvidenceMismatch { line: 6 })
    );
}

#[test]
fn expiry_must_name_a_later_evidence_row() {
    // Seq 5 is an ordinary relay row, not evidence.
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[1] = expire(spec(2), 5);
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::ExpiryEvidenceMissing { line: 2 })
    );
    // Evidence must come after the row it expires.
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[2] = expire(spec(3), 3);
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::ExpiryEvidenceMissing { line: 3 })
    );
    // Expired evidence is not evidence.
    let mut specs = retention_history(retention(1, 2, Some(2), Some(4), &[2, 4]));
    let evidence = specs.pop().expect("evidence");
    specs.push(Spec {
        origin: "relay",
        ..expire(evidence, 7)
    });
    specs.push(spec(7));
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::ExpiryEvidenceMissing { line: 2 })
    );
}

#[test]
fn retention_sets_reaching_before_the_anchor_are_reported_unverified() {
    let specs = retention_history(retention(1, 2, Some(2), Some(4), &[2, 4]));
    let built = build(&specs, 0, GENESIS);
    let anchor = Checkpoint {
        epoch: 1,
        seq: 2,
        chain: built.chains[1],
    };
    let report = verify_export(&text(&built.lines[2..]), Anchor::Checkpoint(anchor))
        .expect("partial set is consistent");
    assert_eq!(report.expired, 1);
    assert_eq!(report.unverified_expiry_evidence, 1);
}

#[test]
fn purge_evidence_names_exactly_one_row() {
    let purge = |target: i64, id: Uuid| {
        control(
            2,
            "audit.body.purged",
            json!({
                "target_seq": target,
                "target_event_id": id.to_string(),
                "purge_reason_code": "adapter_defect"
            }),
        )
    };
    let history = |evidence: Spec, expired: &[usize]| {
        let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
        for i in expired {
            let n = u128::try_from(*i + 1).expect("small");
            specs[*i] = expire(spec(n), 6);
        }
        specs.push(evidence);
        build(&specs, 0, GENESIS)
    };
    let ok = history(purge(3, event_id(3)), &[2]);
    let report = verify_export(&text(&ok.lines), Anchor::Genesis).expect("purge verified");
    assert_eq!(report.unverified_expiry_evidence, 0);
    for (label, built) in [
        ("other event id", history(purge(3, event_id(4)), &[2])),
        ("other target", history(purge(4, event_id(3)), &[2])),
        ("two rows", history(purge(3, event_id(3)), &[2, 3])),
        ("target not expired", history(purge(3, event_id(3)), &[])),
    ] {
        assert_eq!(
            verify_export(&text(&built.lines), Anchor::Genesis),
            Err(ExportError::ExpiryEvidenceMismatch { line: 6 }),
            "{label}"
        );
    }
}

#[test]
fn control_events_never_expire() {
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    let mut verified = control(3, "audit.integrity.verified", json!({}));
    verified = expire(verified, 99);
    specs[2] = verified;
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::ExpiryOnControlEvent { line: 3 })
    );
    assert_eq!(
        verify_export_subset(&text(&built.lines)),
        Err(ExportError::ExpiryOnControlEvent { line: 3 })
    );
}

#[test]
fn line_origin_must_match_the_body() {
    let relabel = |mut s: Spec, origin: &'static str| {
        s.origin = origin;
        s
    };
    let cases: Vec<(&str, Spec)> = vec![
        (
            "relay line, store body",
            relabel(control(4, "audit.integrity.verified", json!({})), "relay"),
        ),
        ("store line, relay body", relabel(spec(3), "store")),
        (
            "relay_control line, store body",
            relabel(control(4, "audit.body.purged", json!({})), "relay_control"),
        ),
        (
            "missing source",
            body_spec(
                event_id(3),
                "relay",
                &json!({"id": event_id(3).to_string(), "type": "document.created"}),
            ),
        ),
        (
            "control source on an unknown relay type",
            body_spec(
                event_id(3),
                "relay",
                &json!({"id": event_id(3).to_string(), "type": "future.thing", "source": AUDIT_STORE_SOURCE}),
            ),
        ),
    ];
    for (label, replacement) in cases {
        let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
        specs[2] = replacement;
        let built = build(&specs, 0, GENESIS);
        assert_eq!(
            verify_export(&text(&built.lines), Anchor::Genesis),
            Err(ExportError::OriginMismatch { line: 3 }),
            "{label}"
        );
    }
    // Types unknown to this catalog fall back to the structural rule.
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[2] = body_spec(
        event_id(3),
        "relay",
        &json!({"id": event_id(3).to_string(), "type": "future.thing", "source": DOCUMENT_SOURCE}),
    );
    let built = build(&specs, 0, GENESIS);
    assert!(verify_export(&text(&built.lines), Anchor::Genesis).is_ok());
}

#[test]
fn envelope_id_must_equal_event_id() {
    let (mut specs, _) = five();
    specs[0].event = Uuid::from_u128(42);
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::EventIdMismatch { line: 1 })
    );
}

#[test]
fn identity_chain_export_has_no_bodies() {
    let (mut specs, built_with_bodies) = five();
    for (i, s) in specs.iter_mut().enumerate() {
        s.body = None;
        s.expired_by = (i == 3).then_some(99);
    }
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        built.chains, built_with_bodies.chains,
        "chain does not depend on bodies"
    );
    let report =
        verify_identity_chain(&text(&built.lines), Anchor::Genesis).expect("identity chain");
    assert!(report.anchored);
    assert_eq!(report.bodies, 0);
    assert_eq!(report.expired, 1);
    assert_eq!(report.unverified_expiry_evidence, 1);
    assert!(!report.epochs_authenticated);
    assert_eq!(report.head.chain, built.chains[4]);
    // Bodies are not allowed in identity-chain mode.
    assert_eq!(
        verify_identity_chain(&text(&built_with_bodies.lines), Anchor::Genesis),
        Err(ExportError::BodyState { line: 1 })
    );
}

#[test]
fn checkpoint_anchor_and_comparisons() {
    let (specs, built) = five();
    let at_2 = Checkpoint {
        epoch: 1,
        seq: 2,
        chain: built.chains[1],
    };
    let tail = &built.lines[2..];
    let report =
        verify_export(&text(tail), Anchor::Checkpoint(at_2)).expect("verifies from checkpoint");
    assert!(report.anchored);
    assert_eq!(report.rows, 3);
    assert_eq!(report.head.seq, 5);

    let head = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: built.chains[4],
    };
    assert_eq!(compare_checkpoint(&report, &head), Cmp::Match);
    let wrong = Checkpoint {
        chain: [7; 32],
        ..head
    };
    assert_eq!(compare_checkpoint(&report, &wrong), Cmp::Mismatch);
    let later = Checkpoint {
        epoch: 1,
        seq: 9,
        chain: [1; 32],
    };
    assert_eq!(compare_checkpoint(&report, &later), Cmp::StoreBehind);
    let at_4 = Checkpoint {
        epoch: 1,
        seq: 4,
        chain: built.chains[3],
    };
    assert_eq!(compare_checkpoint(&report, &at_4), Cmp::Ahead);
    let at_4_wrong = Checkpoint {
        chain: built.chains[2],
        ..at_4
    };
    assert_eq!(compare_checkpoint(&report, &at_4_wrong), Cmp::Mismatch);
    assert_eq!(compare_checkpoint(&report, &at_2), Cmp::Ahead);
    let at_1 = Checkpoint {
        epoch: 1,
        seq: 1,
        chain: built.chains[0],
    };
    assert_eq!(compare_checkpoint(&report, &at_1), Cmp::BeforeAnchor);

    // The wrong anchor chain is a broken link.
    let bad_anchor = Checkpoint {
        chain: [3; 32],
        ..at_2
    };
    assert_eq!(
        verify_export(&text(tail), Anchor::Checkpoint(bad_anchor)),
        Err(ExportError::BrokenLink { line: 1 })
    );
    // Starting at the wrong seq is a gap.
    assert_eq!(
        verify_export(&text(&built.lines[3..]), Anchor::Checkpoint(at_2)),
        Err(ExportError::SeqGap {
            line: 1,
            expected: 3,
            found: 4
        })
    );
    drop(specs);
}

#[test]
fn filtered_subsets_are_never_anchored() {
    let (_, built) = five();
    let subset = vec![
        built.lines[0].clone(),
        built.lines[2].clone(),
        built.lines[4].clone(),
    ];
    let report = verify_export_subset(&text(&subset)).expect("rows are self-consistent");
    assert!(!report.anchored);
    assert!(!report.epochs_authenticated);
    assert_eq!(report.rows, 3);
    let head = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: built.chains[4],
    };
    assert_eq!(compare_checkpoint(&report, &head), Cmp::Unanchored);
    assert!(verify_export(&text(&subset), Anchor::Genesis).is_err());

    let mut tampered = subset.clone();
    tampered[1] = tampered[1].replace(r#"{"n": 3}"#, r#"{"n": 5}"#);
    assert_eq!(
        verify_export_subset(&text(&tampered)),
        Err(ExportError::DigestMismatch { line: 2 })
    );
}

fn epoch_started(
    n: u128,
    old: i64,
    new: i64,
    restored_seq: i64,
    restored_chain: &[u8; 32],
) -> Spec {
    let mut s = control(
        n,
        "audit.recovery.epoch_started",
        json!({
            "old_epoch": old,
            "new_epoch": new,
            "restored_head_seq": restored_seq,
            "restored_head_chain": to_hex(restored_chain),
            "classification": "restore"
        }),
    );
    s.epoch = new;
    s
}

/// Rows 1..=3 in epoch 1, then `transition` at seq 4 and one more row.
fn epoch_history(transition: impl FnOnce(&[[u8; 32]]) -> Spec) -> Built {
    let mut specs: Vec<Spec> = (1..=3).map(spec).collect();
    let prefix = build(&specs, 0, GENESIS);
    specs.push(transition(&prefix.chains));
    let mut after = spec(5);
    after.epoch = specs[3].epoch;
    specs.push(after);
    build(&specs, 0, GENESIS)
}

#[test]
fn epoch_changes_must_be_attested_by_epoch_started() {
    let built = epoch_history(|chains| epoch_started(1, 1, 2, 3, &chains[2]));
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    assert!(report.epochs_authenticated);
    assert_eq!(
        report.epoch_transitions(),
        &[EpochTransition {
            seq: 4,
            old_epoch: 1,
            new_epoch: 2
        }]
    );
    assert_eq!(report.head.epoch, 2);
    assert_eq!(report.epoch_at(0), Some(1));
    assert_eq!(report.epoch_at(3), Some(1));
    assert_eq!(report.epoch_at(4), Some(2));
    assert_eq!(report.epoch_at(5), Some(2));
    assert_eq!(report.epoch_at(6), None);
    // A restored head earlier than the transition (writes after the restore
    // but before detection) is allowed when its chain is on the path.
    let built = epoch_history(|chains| epoch_started(1, 1, 2, 1, &chains[0]));
    assert!(verify_export(&text(&built.lines), Anchor::Genesis).is_ok());

    let unattested: Vec<(&str, Built)> = vec![
        (
            "relay row",
            epoch_history(|_| Spec {
                epoch: 2,
                ..spec(4)
            }),
        ),
        (
            "wrong new epoch",
            epoch_history(|chains| {
                let mut s = epoch_started(1, 1, 3, 3, &chains[2]);
                s.epoch = 2;
                s
            }),
        ),
        (
            "wrong old epoch",
            epoch_history(|chains| {
                let mut s = epoch_started(1, 0, 2, 3, &chains[2]);
                s.epoch = 2;
                s
            }),
        ),
        (
            "restored head chain",
            epoch_history(|chains| epoch_started(1, 1, 2, 3, &chains[1])),
        ),
        (
            "restored head after",
            epoch_history(|chains| epoch_started(1, 1, 2, 4, &chains[2])),
        ),
        (
            "other control type",
            epoch_history(|_| Spec {
                epoch: 2,
                ..control(
                    1,
                    "audit.integrity.verified",
                    json!({"old_epoch": 1, "new_epoch": 2}),
                )
            }),
        ),
    ];
    for (label, built) in unattested {
        assert_eq!(
            verify_export(&text(&built.lines), Anchor::Genesis),
            Err(ExportError::UnattestedEpochChange { line: 4 }),
            "{label}"
        );
    }
}

#[test]
fn epochs_advance_by_exactly_one_and_never_regress() {
    let (mut specs, _) = five();
    specs[3].epoch = 3;
    specs[4].epoch = 3;
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::EpochSkipped { line: 4 })
    );

    let (mut specs, _) = five();
    specs[2].epoch = 2;
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::UnattestedEpochChange { line: 3 })
    );
    let regress = identity_rows(1, 5, 1, GENESIS, 0);
    let mut lines = regress.lines.clone();
    lines[3] = lines[3].replace("\"recovery_epoch\":1", "\"recovery_epoch\":2");
    assert_eq!(
        verify_identity_chain(&text(&lines), Anchor::Genesis),
        Err(ExportError::EpochRegressed { line: 5 })
    );
    let mut skipped = regress.lines;
    skipped[3] = skipped[3].replace("\"recovery_epoch\":1", "\"recovery_epoch\":3");
    assert_eq!(
        verify_identity_chain(&text(&skipped), Anchor::Genesis),
        Err(ExportError::EpochSkipped { line: 4 })
    );
}

#[test]
fn identity_chain_epochs_are_unauthenticated() {
    let original = identity_rows(1, 3, 1, GENESIS, 0);
    let after = identity_rows(4, 5, 2, original.chains[2], 0);
    let mut lines = original.lines.clone();
    lines.extend(after.lines);
    let report = verify_identity_chain(&text(&lines), Anchor::Genesis).expect("consistent");
    assert!(!report.epochs_authenticated);
    assert_eq!(report.epoch_transitions().len(), 1);
}

#[test]
fn checkpoint_epoch_is_compared() {
    let built = identity_rows(1, 10, 1, GENESIS, 0);
    let report = verify_identity_chain(&text(&built.lines), Anchor::Genesis).expect("valid");
    let right = checkpoint_at(&built, 1, 6, 1);
    assert_eq!(compare_checkpoint(&report, &right), Cmp::Ahead);
    let wrong_epoch = Checkpoint { epoch: 2, ..right };
    assert_eq!(
        compare_checkpoint(&report, &wrong_epoch),
        Cmp::EpochMismatch
    );
    let head_wrong_epoch = Checkpoint {
        epoch: 3,
        ..checkpoint_at(&built, 1, 10, 1)
    };
    assert_eq!(
        compare_checkpoint(&report, &head_wrong_epoch),
        Cmp::EpochMismatch
    );
    let assessment = assess_recovery(&report, &[wrong_epoch], &[]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    // The anchor's own epoch is compared too.
    let anchor = checkpoint_at(&built, 1, 4, 1);
    let tail = verify_identity_chain(&text(&built.lines[4..]), Anchor::Checkpoint(anchor))
        .expect("tail verifies");
    assert_eq!(compare_checkpoint(&tail, &anchor), Cmp::Ahead);
    assert_eq!(
        compare_checkpoint(&tail, &Checkpoint { epoch: 5, ..anchor }),
        Cmp::EpochMismatch
    );
}

#[test]
fn seq_overflow_is_reported_not_panicking() {
    let s = spec(1);
    let max = i64::MAX;
    let chain = chain_next(&GENESIS, max, s.event, &s.digest);
    let first = line(max, &s, &GENESIS, &chain);
    let s2 = spec(2);
    let second = line(1, &s2, &chain, &chain_next(&chain, 1, s2.event, &s2.digest));
    assert_eq!(
        verify_export_subset(&text(&[first.clone(), second])),
        Err(ExportError::Malformed { line: 2 })
    );
    let anchor = Checkpoint {
        epoch: 1,
        seq: max,
        chain: GENESIS,
    };
    assert_eq!(
        verify_export(&text(&[first]), Anchor::Checkpoint(anchor)),
        Err(ExportError::Malformed { line: 1 })
    );
}

#[test]
fn malformed_lines_are_rejected() {
    let (_, built) = five();
    let mut cases = Vec::new();
    cases.push((format!("{}\n\n{}", built.lines[0], built.lines[1]), 2));
    cases.push((
        built.lines[0].replace("\"origin\":\"relay\"", "\"origin\":\"elsewhere\""),
        1,
    ));
    cases.push((
        built.lines[0].replace("\"expired\":false", "\"expired\":false,\"extra\":1"),
        1,
    ));
    cases.push((
        built.lines[0].replace("\"seq\":1", "\"seq\":1,\"seq\":1"),
        1,
    ));
    cases.push((
        built.lines[0].replace("\"recovery_epoch\":1", "\"recovery_epoch\":0"),
        1,
    ));
    cases.push(("not json".to_owned(), 1));
    let upper = to_hex(&built.chains[0]);
    cases.push((built.lines[0].replace(&upper, &upper.to_uppercase()), 1));
    for (input, line) in cases {
        assert_eq!(
            verify_export(&input, Anchor::Genesis),
            Err(ExportError::Malformed { line }),
            "{input}"
        );
    }
}

/// Identity-chain rows (no body) for seq `from..=to` in `epoch`, continuing
/// from `prev`.
fn identity_rows(from: i64, to: i64, epoch: i64, prev: [u8; 32], salt: u128) -> Built {
    let specs: Vec<Spec> = (from..=to)
        .map(|seq| {
            let mut s = spec(u128::try_from(seq).expect("positive") | (salt << 32));
            s.body = None;
            s.epoch = epoch;
            s
        })
        .collect();
    build(&specs, from - 1, prev)
}

fn checkpoint_at(built: &Built, from: i64, seq: i64, epoch: i64) -> Checkpoint {
    Checkpoint {
        epoch,
        seq,
        chain: built.chains[usize::try_from(seq - from).expect("in range")],
    }
}

/// History 1..=10 in epoch 1 with an out-of-band checkpoint at 10, then a
/// restore to head 7 and a recovery epoch 2 continuing from seq 8 to 11.
fn restored_history() -> (String, Checkpoint) {
    let original = identity_rows(1, 10, 1, GENESIS, 0);
    let checkpoint = checkpoint_at(&original, 1, 10, 1);
    let survived = &original.lines[..7];
    let after = identity_rows(8, 11, 2, original.chains[6], 0xbeef);
    let mut lines = survived.to_vec();
    lines.extend(after.lines);
    (text(&lines), checkpoint)
}

#[test]
fn honest_history_with_a_matching_checkpoint_is_authentic() {
    let built = identity_rows(1, 10, 1, GENESIS, 0);
    let report = verify_identity_chain(&text(&built.lines), Anchor::Genesis).expect("valid");
    assert!(report.epoch_transitions().is_empty());
    let assessment = assess_recovery(&report, &[checkpoint_at(&built, 1, 10, 1)], &[]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    let ahead = assess_recovery(&report, &[checkpoint_at(&built, 1, 6, 1)], &[]);
    assert_eq!(ahead.verdict, ChainVerdict::Authentic);
    assert_eq!(
        assess_recovery(&report, &[], &[]).verdict,
        ChainVerdict::NoCheckpoint
    );
}

#[test]
fn truncated_suffix_with_fabricated_epoch_and_no_record_is_unverified_recovery() {
    let (export, checkpoint) = restored_history();
    // The fabricated recovery is internally consistent: it verifies.
    let report = verify_identity_chain(&export, Anchor::Genesis).expect("chain is consistent");
    assert_eq!(
        report.epoch_transitions(),
        &[EpochTransition {
            seq: 8,
            old_epoch: 1,
            new_epoch: 2
        }]
    );
    let assessment = assess_recovery(&report, &[checkpoint], &[]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    assert_ne!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(assessment.epochs.len(), 1);
    assert_eq!(assessment.epochs[0].record, None);
    assert_eq!(assessment.findings[0].comparison, Cmp::Mismatch);
}

#[test]
fn documented_recovery_is_reported_as_lost_never_authentic() {
    let (export, checkpoint) = restored_history();
    let report = verify_identity_chain(&export, Anchor::Genesis).expect("valid");
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 7,
        restored_head_chain: report.chain_at(7).expect("on path"),
        lost_upper: 10,
    };
    let assessment = assess_recovery(&report, &[checkpoint], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Lost);
    assert_eq!(assessment.findings[0].explained_by, Some(record));
    assert_eq!(assessment.epochs[0].record, Some(record));
    // A record whose restored head is not on the verified path does not
    // explain it.
    for wrong in [
        RecoveryRecord {
            restored_head_chain: report.chain_at(6).expect("on path"),
            ..record
        },
        RecoveryRecord {
            restored_head_seq: 8,
            ..record
        },
        RecoveryRecord {
            new_epoch: 3,
            ..record
        },
    ] {
        let assessment = assess_recovery(&report, &[checkpoint], &[wrong]);
        assert_eq!(
            assessment.verdict,
            ChainVerdict::UnverifiedRecovery,
            "{wrong:?}"
        );
        assert_eq!(assessment.unmatched_records, vec![wrong]);
    }
    // A lost range that does not reach the checkpoint disagrees as well.
    let short = RecoveryRecord {
        lost_upper: 9,
        ..record
    };
    assert_eq!(
        assess_recovery(&report, &[checkpoint], &[short]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
}

#[test]
fn differences_at_or_below_the_restored_head_are_tampering() {
    let original = identity_rows(1, 10, 1, GENESIS, 0);
    // Seq 5 onwards rewritten, then a "recovery" at 8.
    let rewritten = identity_rows(5, 7, 1, original.chains[3], 0xdead);
    let after = identity_rows(8, 9, 2, rewritten.chains[2], 0xbeef);
    let mut lines = original.lines[..4].to_vec();
    lines.extend(rewritten.lines);
    lines.extend(after.lines);
    let report = verify_identity_chain(&text(&lines), Anchor::Genesis).expect("consistent");
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 7,
        restored_head_chain: report.chain_at(7).expect("on path"),
        lost_upper: 10,
    };
    let old_checkpoint = checkpoint_at(&original, 1, 6, 1);
    let assessment = assess_recovery(&report, &[old_checkpoint], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Tampered);
}

#[test]
fn truncation_without_any_recovery_epoch_is_tampering() {
    let original = identity_rows(1, 10, 1, GENESIS, 0);
    let checkpoint = checkpoint_at(&original, 1, 10, 1);
    let report = verify_identity_chain(&text(&original.lines[..7]), Anchor::Genesis)
        .expect("prefix is consistent");
    let assessment = assess_recovery(&report, &[checkpoint], &[]);
    assert_eq!(assessment.findings[0].comparison, Cmp::StoreBehind);
    assert_eq!(assessment.verdict, ChainVerdict::Tampered);
}

#[test]
fn planned_move_with_an_empty_lost_range_is_authentic() {
    let original = identity_rows(1, 7, 1, GENESIS, 0);
    let checkpoint = checkpoint_at(&original, 1, 7, 1);
    let after = identity_rows(8, 9, 2, original.chains[6], 0x77);
    let mut lines = original.lines.clone();
    lines.extend(after.lines);
    let report = verify_identity_chain(&text(&lines), Anchor::Genesis).expect("valid");
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 7,
        restored_head_chain: original.chains[6],
        lost_upper: 7,
    };
    let assessment = assess_recovery(&report, &[checkpoint], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(assessment.epochs[0].record, Some(record));
    assert_eq!(
        assess_recovery(&report, &[checkpoint], &[]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
}

#[test]
fn subsets_are_unanchored_in_assessment() {
    let (_, built) = five();
    let report = verify_export_subset(&text(&built.lines)).expect("subset");
    assert_eq!(
        assess_recovery(&report, &[], &[]).verdict,
        ChainVerdict::Unanchored
    );
}
