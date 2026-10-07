//! Hash-chain vectors (cross-checked by an independent implementation) and
//! out-of-database export verification.

use audit_core::catalog::{AUDIT_STORE_SOURCE, DOCUMENT_SOURCE};
use audit_core::chain::{GENESIS_PREIMAGE, parse_hex32, to_hex};
use audit_core::{
    Anchor, ChainIntegrity, ChainVerdict, Checkpoint, CheckpointComparison as Cmp,
    EpochAttestation, EpochTransition, ExpiredRowEvidence, ExportError, GENESIS,
    RecoveryClassification, RecoveryRecord, assess_recovery, chain_next, compare_checkpoint,
    envelope_digest, expired_set_digest, verify_export, verify_export_complete,
    verify_export_subset, verify_identity_chain, verify_identity_chain_complete,
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
    assert_eq!(
        report.expired_rows(),
        &[ExpiredRowEvidence {
            seq: 2,
            evidence_seq: 99,
            verified: false
        }]
    );
    // Unverified expiry evidence never yields Authentic, even at a matching
    // head checkpoint.
    let head = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: built.chains[4],
    };
    let assessment = assess_recovery(&report, &[head], &[]);
    assert_eq!(assessment.authenticated_through, Some(5));
    assert_eq!(assessment.unconfirmed_expiries, 1);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedExpiry);

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
    let verified = |seq| ExpiredRowEvidence {
        seq,
        evidence_seq: 6,
        verified: true,
    };
    assert_eq!(report.expired_rows(), &[verified(2), verified(4)]);

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
    // Expiry cannot be confirmed without bodies, so a matching head
    // checkpoint still does not make the identity chain Authentic.
    let head = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: built.chains[4],
    };
    let assessment = assess_recovery(&report, &[head], &[]);
    assert_eq!(assessment.unconfirmed_expiries, 1);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedExpiry);
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

/// An `audit.recovery.epoch_started` body with explicit details.
fn epoch_details(n: u128, old: i64, new: i64, details: Value) -> Spec {
    let mut details = details;
    details["old_epoch"] = json!(old);
    details["new_epoch"] = json!(new);
    let mut s = control(n, "audit.recovery.epoch_started", details);
    s.epoch = new;
    s
}

/// An `audit.recovery.epoch_started` body with the lost range
/// `(restored_seq, lost_upper]`.
fn epoch_body(
    n: u128,
    old: i64,
    new: i64,
    restored: (i64, &[u8; 32]),
    lost_upper: i64,
    classification: &str,
) -> Spec {
    epoch_details(
        n,
        old,
        new,
        json!({
            "restored_head_seq": restored.0,
            "restored_head_chain": to_hex(restored.1),
            "lost_from_seq": restored.0.wrapping_add(1),
            "lost_upper_seq": lost_upper,
            "classification": classification
        }),
    )
}

/// A restore whose lost range is empty (`lost_upper = restored_seq`).
fn epoch_started(
    n: u128,
    old: i64,
    new: i64,
    restored_seq: i64,
    restored_chain: &[u8; 32],
) -> Spec {
    epoch_body(
        n,
        old,
        new,
        (restored_seq, restored_chain),
        restored_seq,
        "restore",
    )
}

fn attestation(
    restored: (i64, [u8; 32]),
    lost_upper_seq: i64,
    classification: RecoveryClassification,
) -> Option<EpochAttestation> {
    Some(EpochAttestation {
        restored_head_seq: restored.0,
        restored_head_chain: restored.1,
        lost_from_seq: restored.0 + 1,
        lost_upper_seq,
        classification,
    })
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
    assert_eq!(report.unverified_restored_heads, 0);
    assert_eq!(
        report.epoch_transitions(),
        &[EpochTransition {
            seq: 4,
            old_epoch: 1,
            new_epoch: 2,
            attestation: attestation((3, built.chains[2]), 3, RecoveryClassification::Restore),
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
            // P4: a negative restored head would skip the chain comparison.
            "negative restored head",
            epoch_history(|_| epoch_started(1, 1, 2, -5, &[7; 32])),
        ),
        (
            "minimal restored head",
            epoch_history(|_| epoch_started(1, 1, 2, i64::MIN, &[7; 32])),
        ),
        (
            "lost upper below the restored head",
            epoch_history(|chains| epoch_body(1, 1, 2, (3, &chains[2]), 2, "restore")),
        ),
        (
            "unknown classification",
            epoch_history(|chains| epoch_body(1, 1, 2, (3, &chains[2]), 3, "rebind")),
        ),
        (
            "lost range not starting after the restored head",
            epoch_history(|chains| {
                epoch_details(
                    1,
                    1,
                    2,
                    json!({
                        "restored_head_seq": 3,
                        "restored_head_chain": to_hex(&chains[2]),
                        "lost_from_seq": 9,
                        "lost_upper_seq": 9,
                        "classification": "restore"
                    }),
                )
            }),
        ),
        (
            "missing lost range",
            epoch_history(|chains| {
                epoch_details(
                    1,
                    1,
                    2,
                    json!({
                        "restored_head_seq": 3,
                        "restored_head_chain": to_hex(&chains[2]),
                        "classification": "restore"
                    }),
                )
            }),
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
    assert_eq!(report.epoch_transitions()[0].attestation, None);
}

#[test]
fn epoch_started_bodies_must_sit_on_a_transition() {
    // P1 / S4: the unchained epoch column of the recovery rows is rewritten
    // back to the old epoch, so no transition would be listed.
    let flattened = epoch_history(|chains| {
        let mut s = epoch_started(1, 1, 2, 3, &chains[2]);
        s.epoch = 1;
        s
    });
    assert_eq!(
        verify_export(&text(&flattened.lines), Anchor::Genesis),
        Err(ExportError::EpochStartedWithoutTransition { line: 4 })
    );
    let anchor = Checkpoint {
        epoch: 1,
        seq: 2,
        chain: flattened.chains[1],
    };
    assert_eq!(
        verify_export(&text(&flattened.lines[2..]), Anchor::Checkpoint(anchor)),
        Err(ExportError::EpochStartedWithoutTransition { line: 2 })
    );
    // A filtered subset attests nothing and is not refused for it.
    assert!(verify_export_subset(&text(&flattened.lines)).is_ok());
}

#[test]
fn restored_heads_before_the_anchor_are_counted_unverified() {
    // Restored head 1, transition at 4 (rows 2..=3 were written after the
    // restore and are inside the lost range).
    let built = epoch_history(|chains| epoch_body(1, 1, 2, (1, &chains[0]), 3, "restore"));
    let full = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    assert_eq!(full.unverified_restored_heads, 0);
    let anchor = Checkpoint {
        epoch: 1,
        seq: 2,
        chain: built.chains[1],
    };
    let tail = verify_export(&text(&built.lines[2..]), Anchor::Checkpoint(anchor))
        .expect("consistent from the anchor");
    assert_eq!(tail.unverified_restored_heads, 1);
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 1,
        restored_head_chain: built.chains[0],
        lost_upper: 3,
    };
    let head = Checkpoint {
        epoch: 2,
        seq: 5,
        chain: built.chains[4],
    };
    let from_anchor = assess_recovery(&tail, &[head], &[record]);
    assert_eq!(from_anchor.epochs[0].record, Some(record));
    assert!(!from_anchor.epochs[0].restored_head_verified);
    assert_eq!(from_anchor.verdict, ChainVerdict::UnverifiedRecovery);
    let from_genesis = assess_recovery(&full, &[head], &[record]);
    assert!(from_genesis.epochs[0].restored_head_verified);
    assert_eq!(from_genesis.verdict, ChainVerdict::Lost);
}

/// Bodies 1..=3 in epoch 1, then `rows` (epoch 1 rows forged after the
/// restored head, then `transition`, then one epoch-2 row).
fn recovery_export(
    forged_old_epoch_rows: u128,
    transition: impl FnOnce(&[[u8; 32]]) -> Spec,
) -> Built {
    let mut specs: Vec<Spec> = (1..=3).map(spec).collect();
    let prefix = build(&specs, 0, GENESIS);
    for n in 0..forged_old_epoch_rows {
        specs.push(spec(0x100 + n));
    }
    let attested = transition(&prefix.chains);
    let epoch = attested.epoch;
    specs.push(attested);
    let mut after = spec(0x200);
    after.epoch = epoch;
    specs.push(after);
    build(&specs, 0, GENESIS)
}

fn head_checkpoint(built: &Built, epoch: i64) -> Checkpoint {
    let seq = i64::try_from(built.chains.len()).expect("small");
    Checkpoint {
        epoch,
        seq,
        chain: built.chains[built.chains.len() - 1],
    }
}

#[test]
fn records_must_agree_with_the_chained_epoch_started_body() {
    // P3: the body says restored head 3 (planned move), transition at 4.
    let built = recovery_export(0, |chains| {
        epoch_body(1, 1, 2, (3, &chains[2]), 3, "planned_move")
    });
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    let head = head_checkpoint(&built, 2);
    let honest = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 3,
        restored_head_chain: built.chains[2],
        lost_upper: 3,
    };
    let assessment = assess_recovery(&report, &[head], &[honest]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(assessment.epochs[0].record, Some(honest));
    // A planned move documented at an earlier head contradicts the body and
    // leaves rows 2..=3 outside its (empty) lost range.
    let earlier = RecoveryRecord {
        restored_head_seq: 1,
        restored_head_chain: built.chains[0],
        lost_upper: 1,
        ..honest
    };
    let assessment = assess_recovery(&report, &[head], &[earlier]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    assert_eq!(assessment.epochs[0].record, None);
    assert_eq!(assessment.unmatched_records, vec![earlier]);
    // A lost range that differs from the body does not match either.
    let wider = RecoveryRecord {
        lost_upper: 4,
        ..honest
    };
    assert_eq!(
        assess_recovery(&report, &[head], &[wider]).verdict,
        ChainVerdict::UnverifiedRecovery
    );

    // P7: forged epoch-1 rows at 4..=5, then the planned move at 6.
    let built = recovery_export(2, |chains| {
        epoch_body(1, 1, 2, (3, &chains[2]), 3, "planned_move")
    });
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    let assessment = assess_recovery(&report, &[head_checkpoint(&built, 2)], &[honest]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    assert_eq!(assessment.epochs[0].transition.seq, 6);
    assert_eq!(assessment.epochs[0].record, None);

    // A restore whose body and record agree on a lost range covering the
    // rows written after the restored head is Lost, never Authentic.
    let built = recovery_export(2, |chains| {
        epoch_body(1, 1, 2, (3, &chains[2]), 5, "restore")
    });
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    let record = RecoveryRecord {
        lost_upper: 5,
        ..honest
    };
    let assessment = assess_recovery(&report, &[head_checkpoint(&built, 2)], &[record]);
    assert_eq!(assessment.epochs[0].record, Some(record));
    assert_eq!(assessment.verdict, ChainVerdict::Lost);

    // A "planned move" body that claims a non-empty lost range is not a
    // planned move: no record explains it.
    let built = recovery_export(2, |chains| {
        epoch_body(1, 1, 2, (3, &chains[2]), 5, "planned_move")
    });
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("attested");
    assert_eq!(
        assess_recovery(&report, &[head_checkpoint(&built, 2)], &[record]).verdict,
        ChainVerdict::UnverifiedRecovery
    );
}

/// Rows 1..=3 in epoch 1, a planned move (restored head 3) at seq 4, rows
/// 5..=8 in epoch 2.
fn moved_history() -> (Built, RecoveryRecord) {
    let mut specs: Vec<Spec> = (1..=3).map(spec).collect();
    let prefix = build(&specs, 0, GENESIS);
    specs.push(epoch_body(
        1,
        1,
        2,
        (3, &prefix.chains[2]),
        3,
        "planned_move",
    ));
    for n in 5..=8 {
        let mut s = spec(n);
        s.epoch = 2;
        specs.push(s);
    }
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 3,
        restored_head_chain: prefix.chains[2],
        lost_upper: 3,
    };
    (build(&specs, 0, GENESIS), record)
}

#[test]
fn records_and_checkpoints_before_the_anchor_are_neutral() {
    let (built, record) = moved_history();
    let cp = |seq: i64, epoch: i64| checkpoint_at(&built, 1, seq, epoch);
    // P2a: full export from genesis.
    let full = verify_export(&text(&built.lines), Anchor::Genesis).expect("valid");
    assert_eq!(
        assess_recovery(&full, &[cp(8, 2)], &[record]).verdict,
        ChainVerdict::Authentic
    );
    // P2b: tail from the checkpoint at 6, no records.
    let tail = verify_export(&text(&built.lines[6..]), Anchor::Checkpoint(cp(6, 2)))
        .expect("tail verifies");
    assert_eq!(
        assess_recovery(&tail, &[cp(8, 2)], &[]).verdict,
        ChainVerdict::Authentic
    );
    // P2c: the same tail with the full out-of-band log.
    let assessment = assess_recovery(&tail, &[cp(8, 2)], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert!(assessment.unmatched_records.is_empty());
    assert_eq!(assessment.records_before_anchor, vec![record]);
    // P2d: a checkpoint before the anchor confirms nothing but is not a
    // finding against the export.
    let assessment = assess_recovery(&tail, &[cp(2, 1), cp(8, 2)], &[]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(assessment.findings[0].comparison, Cmp::BeforeAnchor);
    assert_eq!(assessment.authenticated_through, Some(8));
    // Only before-anchor checkpoints: nothing confirms the path.
    assert_eq!(
        assess_recovery(&tail, &[cp(2, 1)], &[]).verdict,
        ChainVerdict::NoCheckpoint
    );
    // A record of a transition after the verified head (it leaves the
    // head's epoch 2) cannot be on this path: neutral, listed for review.
    let later = RecoveryRecord {
        old_epoch: 2,
        new_epoch: 3,
        restored_head_seq: 7,
        restored_head_chain: built.chains[6],
        lost_upper: 7,
    };
    let assessment = assess_recovery(&tail, &[cp(8, 2)], &[later]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert!(assessment.unmatched_records.is_empty());
    assert_eq!(assessment.records_after_head, vec![later]);
    // A record of a transition the path does cross must match it: the full
    // export crosses 1 -> 2 at seq 4.
    let wrong_head = RecoveryRecord {
        restored_head_seq: 2,
        restored_head_chain: built.chains[1],
        lost_upper: 3,
        ..record
    };
    let assessment = assess_recovery(&full, &[cp(8, 2)], &[wrong_head, later]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    assert_eq!(assessment.unmatched_records, vec![wrong_head]);
    assert_eq!(assessment.records_after_head, vec![later]);
}

#[test]
fn checkpoints_at_the_anchor_are_neutral() {
    let (built, _) = moved_history();
    let cp = |seq: i64, epoch: i64| checkpoint_at(&built, 1, seq, epoch);
    let anchor = cp(6, 2);
    let tail =
        verify_export(&text(&built.lines[6..]), Anchor::Checkpoint(anchor)).expect("tail verifies");
    // The anchor itself as a checkpoint confirms nothing new: the factual
    // comparison is kept, the verdict is neutral.
    let assessment = assess_recovery(&tail, &[anchor], &[]);
    assert_eq!(compare_checkpoint(&tail, &anchor), Cmp::Ahead);
    assert_eq!(assessment.findings[0].comparison, Cmp::Ahead);
    assert_eq!(assessment.findings[0].verdict, ChainVerdict::NoCheckpoint);
    assert_eq!(assessment.authenticated_through, None);
    assert_eq!(assessment.verdict, ChainVerdict::NoCheckpoint);
    // Another out-of-band entry at the anchor's seq that disagrees with the
    // anchor (chain or epoch) is a conflict between the trusted inputs, not
    // evidence against the exported rows: never Tampered.
    for conflicting in [
        Checkpoint {
            chain: [0x5a; 32],
            ..anchor
        },
        Checkpoint { epoch: 1, ..anchor },
    ] {
        let assessment = assess_recovery(&tail, &[conflicting, cp(8, 2)], &[]);
        assert_ne!(assessment.findings[0].comparison, Cmp::Ahead);
        assert_eq!(assessment.findings[0].verdict, ChainVerdict::NoCheckpoint);
        assert_eq!(
            assessment.verdict,
            ChainVerdict::Authentic,
            "{conflicting:?}"
        );
        assert_eq!(assessment.authenticated_through, Some(8));
    }
    // An empty export anchored at a checkpoint is not authenticated by the
    // same checkpoint.
    let empty = verify_export("", Anchor::Checkpoint(anchor)).expect("empty");
    assert_eq!(
        assess_recovery(&empty, &[anchor], &[]).verdict,
        ChainVerdict::NoCheckpoint
    );
    // Past the anchor, differences still count.
    let rewritten = Checkpoint {
        chain: [0x5a; 32],
        ..cp(7, 2)
    };
    assert_eq!(
        assess_recovery(&tail, &[rewritten, cp(8, 2)], &[]).verdict,
        ChainVerdict::Tampered
    );
}

#[test]
fn chain_integrity_distinguishes_intact_broken_and_unanchored() {
    let (_, built) = five();
    let full = text(&built.lines);
    let identity: Vec<String> = built
        .lines
        .iter()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).expect("line");
            value["envelope"] = Value::Null;
            value["expired"] = Value::Bool(false);
            value.to_string()
        })
        .collect();
    let identity = text(&identity);
    assert_eq!(
        ChainIntegrity::of(&verify_identity_chain(&identity, Anchor::Genesis)),
        ChainIntegrity::Intact
    );
    assert_eq!(
        ChainIntegrity::of(&verify_export(&full, Anchor::Genesis)),
        ChainIntegrity::Intact
    );
    let gap = text(&[built.lines[0].clone(), built.lines[2].clone()]);
    let broken = ChainIntegrity::of(&verify_export(&gap, Anchor::Genesis));
    assert_eq!(
        broken,
        ChainIntegrity::Broken(ExportError::SeqGap {
            line: 2,
            expected: 2,
            found: 3
        })
    );
    assert_eq!(broken.as_str(), "broken");
    let subset = ChainIntegrity::of(&verify_export_subset(&gap));
    assert_eq!(subset, ChainIntegrity::Unanchored);
    assert_eq!(
        (ChainIntegrity::Intact.as_str(), subset.as_str()),
        ("intact", "unanchored")
    );
    // Intact is about the chain only: an intact identity chain still has
    // unattested epochs and expiries, and no checkpoint makes it authentic.
    let report = verify_identity_chain(&identity, Anchor::Genesis).expect("intact");
    assert!(!report.epochs_authenticated);
    assert_eq!(
        assess_recovery(&report, &[], &[]).verdict,
        ChainVerdict::NoCheckpoint
    );
    // The complete variant also requires the manifest watermark.
    assert!(verify_identity_chain_complete(&identity, Anchor::Genesis, 5).is_ok());
    assert_eq!(
        verify_identity_chain_complete(&identity, Anchor::Genesis, 6),
        Err(ExportError::WatermarkMismatch {
            watermark: 6,
            head: 5
        })
    );
}

#[test]
fn rows_past_the_last_checkpoint_are_authentic_only_through_it() {
    // P9: rows 6..=7 appended after the checkpoint at 5 extend the public
    // chain without any secret.
    let specs: Vec<Spec> = (1..=7).map(spec).collect();
    let built = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("valid");
    let at_5 = checkpoint_at(&built, 1, 5, 1);
    let assessment = assess_recovery(&report, &[at_5], &[]);
    assert_eq!(
        assessment.verdict,
        ChainVerdict::AuthenticThrough { seq: 5 }
    );
    assert_eq!(assessment.authenticated_through, Some(5));
    assert_eq!(
        assessment.findings[0].verdict,
        ChainVerdict::AuthenticThrough { seq: 5 }
    );
    let at_7 = checkpoint_at(&built, 1, 7, 1);
    let assessment = assess_recovery(&report, &[at_5, at_7], &[]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(assessment.authenticated_through, Some(7));
    // The same for identity-chain exports.
    let identity = identity_rows(1, 7, 1, GENESIS, 0);
    let report = verify_identity_chain(&text(&identity.lines), Anchor::Genesis).expect("valid");
    assert_eq!(
        assess_recovery(&report, &[checkpoint_at(&identity, 1, 5, 1)], &[]).verdict,
        ChainVerdict::AuthenticThrough { seq: 5 }
    );
    // Partial authenticity ranks between Authentic and NoCheckpoint.
    assert!(ChainVerdict::Authentic < ChainVerdict::AuthenticThrough { seq: 1 });
    assert!(ChainVerdict::AuthenticThrough { seq: i64::MAX } < ChainVerdict::UnverifiedExpiry);
    assert!(ChainVerdict::UnverifiedExpiry < ChainVerdict::NoCheckpoint);
}

#[test]
fn expiry_evidence_past_the_last_checkpoint_is_not_confirmed() {
    // P1: honest rows 1..=5 and an out-of-band checkpoint at 5.
    let (_, honest) = five();
    let checkpoint = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: honest.chains[4],
    };
    // Row 3's body is removed and retention "evidence" is appended after the
    // checkpoint, chained from the public chain at 5.
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[2] = expire(spec(3), 6);
    specs.push(retention(1, 1, Some(3), Some(3), &[3]));
    let forged = build(&specs, 0, GENESIS);
    assert_eq!(forged.chains[..5], honest.chains[..]);
    let report = verify_export(&text(&forged.lines), Anchor::Genesis).expect("self-consistent");
    assert_eq!(report.unverified_expiry_evidence, 0);
    assert_eq!(
        report.expired_rows(),
        &[ExpiredRowEvidence {
            seq: 3,
            evidence_seq: 6,
            verified: true
        }]
    );
    let assessment = assess_recovery(&report, &[checkpoint], &[]);
    assert_eq!(assessment.authenticated_through, Some(5));
    assert_eq!(assessment.unconfirmed_expiries, 1);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedExpiry);
    // The same retention covered by a checkpoint at the head is confirmed.
    let at_head = Checkpoint {
        epoch: 1,
        seq: 6,
        chain: forged.chains[5],
    };
    let assessment = assess_recovery(&report, &[checkpoint, at_head], &[]);
    assert_eq!(assessment.authenticated_through, Some(6));
    assert_eq!(assessment.unconfirmed_expiries, 0);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    // Expired rows that lie past the checkpoint are already covered by
    // AuthenticThrough.
    let mut specs: Vec<Spec> = (1..=7).map(spec).collect();
    specs[5] = expire(spec(6), 7);
    specs[6] = retention(1, 1, Some(6), Some(6), &[6]);
    let suffix = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&suffix.lines), Anchor::Genesis).expect("valid");
    let assessment = assess_recovery(&report, &[checkpoint_at(&suffix, 1, 5, 1)], &[]);
    assert_eq!(assessment.unconfirmed_expiries, 0);
    assert_eq!(
        assessment.verdict,
        ChainVerdict::AuthenticThrough { seq: 5 }
    );
}

#[test]
fn relabelled_control_rows_cannot_hide_behind_missing_evidence() {
    // S3: seq 3 is an origin=store intent; the forged export relabels it as
    // relay, removes its body and points expired_by_seq past the head.
    let intent = || {
        control(
            3,
            "audit.access.intent_opened",
            json!({"operation": "export"}),
        )
    };
    let mut specs: Vec<Spec> = (1..=5).map(spec).collect();
    specs[2] = intent();
    let honest = build(&specs, 0, GENESIS);
    let checkpoint = Checkpoint {
        epoch: 1,
        seq: 5,
        chain: honest.chains[4],
    };
    specs[2] = Spec {
        origin: "relay",
        ..expire(intent(), i64::MAX)
    };
    let forged = build(&specs, 0, GENESIS);
    assert_eq!(forged.chains, honest.chains);
    let report = verify_export(&text(&forged.lines), Anchor::Genesis).expect("consistent");
    assert_eq!(report.unverified_expiry_evidence, 1);
    let assessment = assess_recovery(&report, &[checkpoint], &[]);
    assert_eq!(assessment.authenticated_through, Some(5));
    assert_eq!(assessment.unconfirmed_expiries, 1);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedExpiry);
    // A complete export up to the manifest watermark cannot name later
    // evidence.
    assert_eq!(
        verify_export_complete(&text(&forged.lines), Anchor::Genesis, 5),
        Err(ExportError::ExpiryEvidenceMissing { line: 3 })
    );
    let report = verify_export_complete(&text(&honest.lines), Anchor::Genesis, 5)
        .expect("honest complete export");
    assert_eq!(
        assess_recovery(&report, &[checkpoint], &[]).verdict,
        ChainVerdict::Authentic
    );
}

#[test]
fn complete_exports_end_exactly_at_the_watermark() {
    let (_, built) = five();
    let report = verify_export_complete(&text(&built.lines), Anchor::Genesis, 5).expect("complete");
    assert_eq!(report.head.seq, 5);
    assert!(report.anchored);
    for (lines, watermark, head) in [(&built.lines[..], 6, 5), (&built.lines[..4], 5, 4)] {
        assert_eq!(
            verify_export_complete(&text(lines), Anchor::Genesis, watermark),
            Err(ExportError::WatermarkMismatch { watermark, head })
        );
    }
    assert_eq!(
        verify_export_complete(&text(&built.lines), Anchor::Genesis, 4),
        Err(ExportError::WatermarkMismatch {
            watermark: 4,
            head: 5
        })
    );
    // Evidence inside the export is verified as usual.
    let specs = retention_history(retention(1, 2, Some(2), Some(4), &[2, 4]));
    let built = build(&specs, 0, GENESIS);
    let report =
        verify_export_complete(&text(&built.lines), Anchor::Genesis, 6).expect("evidence in range");
    assert_eq!(report.unverified_expiry_evidence, 0);
    // Evidence past the watermark is missing evidence.
    let (mut specs, _) = five();
    specs[1] = expire(spec(2), 6);
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export_complete(&text(&built.lines), Anchor::Genesis, 5),
        Err(ExportError::ExpiryEvidenceMissing { line: 2 })
    );
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
    assert_eq!(assessment.authenticated_through, Some(10));
    // A checkpoint behind the head authenticates only up to its own seq
    // (design §8: the end point must equal an out-of-band checkpoint).
    let ahead = assess_recovery(&report, &[checkpoint_at(&built, 1, 6, 1)], &[]);
    assert_eq!(ahead.verdict, ChainVerdict::AuthenticThrough { seq: 6 });
    assert_eq!(ahead.authenticated_through, Some(6));
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
            new_epoch: 2,
            attestation: None,
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
        // Rows 6..=7 lie between the documented restored head and the
        // transition at 8 but outside the documented lost range.
        RecoveryRecord {
            restored_head_seq: 5,
            restored_head_chain: report.chain_at(5).expect("on path"),
            lost_upper: 6,
            ..record
        },
        RecoveryRecord {
            restored_head_seq: -1,
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
    lines.extend(after.lines.iter().cloned());
    let report = verify_identity_chain(&text(&lines), Anchor::Genesis).expect("valid");
    let record = RecoveryRecord {
        old_epoch: 1,
        new_epoch: 2,
        restored_head_seq: 7,
        restored_head_chain: original.chains[6],
        lost_upper: 7,
    };
    // The checkpoint taken before the move authenticates only up to it.
    let assessment = assess_recovery(&report, &[checkpoint], &[record]);
    assert_eq!(
        assessment.verdict,
        ChainVerdict::AuthenticThrough { seq: 7 }
    );
    assert_eq!(assessment.epochs[0].record, Some(record));
    assert!(assessment.epochs[0].restored_head_verified);
    // A checkpoint at the head after the move makes the whole path authentic.
    let after_move = checkpoint_at(&after, 8, 9, 2);
    let assessment = assess_recovery(&report, &[checkpoint, after_move], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Authentic);
    assert_eq!(
        assess_recovery(&report, &[checkpoint, after_move], &[]).verdict,
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
