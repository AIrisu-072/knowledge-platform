//! Hash-chain vectors (cross-checked by an independent implementation) and
//! out-of-database export verification.

use audit_core::chain::{GENESIS_PREIMAGE, parse_hex32, to_hex};
use audit_core::{
    Anchor, ChainVerdict, Checkpoint, CheckpointComparison as Cmp, EpochTransition, ExportError,
    GENESIS, RecoveryRecord, assess_recovery, chain_next, compare_checkpoint, envelope_digest,
    verify_export, verify_export_subset, verify_identity_chain,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const GENESIS_HEX: &str = "9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344";
const VECTOR_EVENT_1: &str = "0199a1b2-0000-7000-8000-000000000001";
const VECTOR_EVENT_2: &str = "0199a1b2-0000-7000-8000-000000000002";
const VECTOR_BODY_1: &str = r#"{"example":"envelope"}"#;
const VECTOR_DIGEST_1: &str = "c147ac99fc21ba6cc69a47812b1bb2415e353995e22b65d6a2b58a5222dd5f6f";
const VECTOR_CHAIN_1: &str = "babbd336ec2c8556082424a253afb6d8e88ef5a63c9cbcab6cea8f441ad528e1";
const VECTOR_CHAIN_2: &str = "1496cf0e490216c2ca995db8ac0a340562a691cf1d2f955865624900b4856106";

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
    expired: bool,
    epoch: i64,
}

fn spec(n: u128) -> Spec {
    let event = Uuid::from_u128(0x0199a1b2_0000_7000_8000_000000000000 | n);
    // PostgreSQL jsonb::text style, hashed byte-for-byte.
    let body = format!(r#"{{"id": "{event}", "data": {{"n": {n}}}, "type": "document.created"}}"#);
    Spec {
        event,
        origin: "relay",
        digest: envelope_digest(&body),
        body: Some(body),
        expired: false,
        epoch: 1,
    }
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
        r#"{{"seq":{seq},"event_id":"{}","origin":"{}","envelope_digest":"{}","prev_chain":"{}","chain":"{}","recovery_epoch":{},"expired":{},"envelope":{}}}"#,
        s.event,
        s.origin,
        to_hex(&s.digest),
        to_hex(prev),
        to_hex(chain),
        s.epoch,
        s.expired,
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
    let forged = format!("{{\"id\": \"{}\", \"forged\": true}}", specs[1].event);
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
    let (mut specs, _) = five();
    specs[1].body = None;
    specs[1].expired = true;
    let built = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("expired row is fine");
    assert_eq!(report.expired, 1);
    assert_eq!(report.bodies, 4);

    let (mut specs, _) = five();
    specs[1].expired = true;
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
        s.expired = i == 3;
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

#[test]
fn epochs_may_advance_but_never_regress() {
    let (mut specs, _) = five();
    specs[3].epoch = 2;
    specs[4].epoch = 2;
    let built = build(&specs, 0, GENESIS);
    let report = verify_export(&text(&built.lines), Anchor::Genesis).expect("epoch advance");
    assert_eq!(report.head.epoch, 2);

    let (mut specs, _) = five();
    specs[2].epoch = 2;
    let built = build(&specs, 0, GENESIS);
    assert_eq!(
        verify_export(&text(&built.lines), Anchor::Genesis),
        Err(ExportError::EpochRegressed { line: 4 })
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
        restored_head: 7,
        lost_to: 10,
    };
    let assessment = assess_recovery(&report, &[checkpoint], &[record]);
    assert_eq!(assessment.verdict, ChainVerdict::Lost);
    assert_eq!(assessment.findings[0].explained_by, Some(record));
    assert_eq!(assessment.epochs[0].record, Some(record));
    // A record that disagrees (wrong restored head) does not explain it.
    let wrong = RecoveryRecord {
        restored_head: 6,
        ..record
    };
    let assessment = assess_recovery(&report, &[checkpoint], &[wrong]);
    assert_eq!(assessment.verdict, ChainVerdict::UnverifiedRecovery);
    assert_eq!(assessment.unmatched_records, vec![wrong]);
    // A lost range that does not reach the checkpoint disagrees as well.
    let short = RecoveryRecord {
        lost_to: 9,
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
        restored_head: 7,
        lost_to: 10,
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
fn subsets_are_unanchored_in_assessment() {
    let (_, built) = five();
    let report = verify_export_subset(&text(&built.lines)).expect("subset");
    assert_eq!(
        assess_recovery(&report, &[], &[]).verdict,
        ChainVerdict::Unanchored
    );
}
