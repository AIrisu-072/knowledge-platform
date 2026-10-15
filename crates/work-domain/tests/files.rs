//! Private work files in the Work-owned shared store, pinned into the Handoff
//! Snapshot on submit, and explicit rework after a return (U3). Authorization
//! stays U1's; the store itself is outside the domain and only its server-side
//! verification receipt is attached.
use std::collections::BTreeSet;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use work_domain::*;

const T0: &str = "2026-10-07T09:00:00Z";
const T1: &str = "2026-10-07T09:05:00Z";
const HASH_A: &str = "a1a2a3a4a5a6a7a8a9a0b1b2b3b4b5b6b7b8b9b0c1c2c3c4c5c6c7c8c9c0d1d2";
const HASH_B: &str = "0f0e0d0c0b0a09080706050403020100f0e0d0c0b0a090807060504030201000";
fn at(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}
fn ctx(acting: Uuid, revision: i64) -> CommandContext {
    CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: acting,
    }
}
fn sales_a(policy: &OrganizationPolicy) -> Workflow {
    Workflow::synthetic(None).with_authority(policy.clone(), at(T0))
}
fn context_b(policy: &OrganizationPolicy) -> Workflow {
    Workflow::from_fixture(
        context_fixture(CONTEXT_B_WORKFLOW_ID).unwrap(),
        None,
        at(T0),
    )
    .unwrap()
    .with_authority(policy.clone(), at(T0))
}
fn claim(w: &mut Workflow, actor: VerifiedActor, acting: Uuid, task_id: Uuid) {
    let revision = w.detail_or_queue_revision(task_id);
    w.apply(
        actor,
        &Command::Claim {
            task_id,
            context: ctx(acting, revision),
        },
        T0,
    )
    .unwrap();
}
fn create(w: &mut Workflow, name: &str) -> WorkingArtifact {
    let MutationResult::ArtifactCreated { artifact, .. } = w
        .apply(
            VerifiedActor::Sales01,
            &Command::CreateFileArtifact {
                task_id: w.source.id,
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
                file_name: name.into(),
                media_type: "text/plain".into(),
            },
            T0,
        )
        .unwrap()
    else {
        panic!("not created")
    };
    artifact
}
fn write_command(w: &Workflow, artifact: &WorkingArtifact, size: u64, hash: &str) -> Command {
    let context = ctx(SALES_ASSIGNMENT_ID, w.source.revision);
    Command::WriteArtifactContent {
        task_id: w.source.id,
        artifact_id: artifact.id,
        generation: GenerationInput {
            id: context.operation_id,
            size_bytes: size,
            sha256: hash.into(),
        },
        context,
        expected_artifact_revision: artifact.revision,
    }
}
fn write(w: &mut Workflow, artifact: &WorkingArtifact, size: u64, hash: &str) -> WorkingArtifact {
    let command = write_command(w, artifact, size, hash);
    let MutationResult::ArtifactContentWritten { artifact, .. } =
        w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!("not written")
    };
    artifact
}
fn text(w: &mut Workflow, value: &str) -> WorkingArtifact {
    let MutationResult::DraftSaved { artifact, .. } = w
        .apply(
            VerifiedActor::Sales01,
            &Command::SaveDraft {
                task_id: w.source.id,
                artifact_id: None,
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
                value: TextValue { text: value.into() },
            },
            T0,
        )
        .unwrap()
    else {
        panic!("not saved")
    };
    artifact
}
fn submit_command(w: &Workflow, artifacts: &[&WorkingArtifact]) -> Command {
    Command::Submit {
        task_id: w.source.id,
        context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
        expected_attempt_id: Some(w.source.attempt_id),
        artifacts: artifacts
            .iter()
            .map(|value| ArtifactSelection {
                artifact_id: value.id,
                revision: value.revision,
            })
            .collect(),
        evidence_revision_refs: vec![],
        finding_revision_refs: vec![],
        decision_revision_refs: vec![],
    }
}
fn generation(artifact: &WorkingArtifact) -> FileGeneration {
    artifact.file.clone().unwrap().generation.unwrap()
}
fn verified(w: &mut Workflow, ids: &[Uuid]) {
    w.attach_verified_generations(ids.iter().copied().collect::<BTreeSet<_>>());
}
trait Revision {
    fn detail_or_queue_revision(&self, id: Uuid) -> i64;
}
impl Revision for Workflow {
    fn detail_or_queue_revision(&self, id: Uuid) -> i64 {
        if self.source.id == id {
            self.source.revision
        } else {
            self.next.as_ref().unwrap().revision
        }
    }
}

#[test]
fn a_private_file_becomes_an_immutable_generation_and_is_pinned_only_with_a_server_receipt() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = sales_a(&policy);
    let created = create(&mut w, "合成_資金計画.txt");
    assert_eq!(created.schema_id, FILE_SCHEMA_ID);
    assert_eq!(created.visibility, "work_item_private");
    assert_eq!(created.value, None);
    let file = created.file.clone().unwrap();
    assert_eq!(
        (file.file_name.as_str(), file.media_type.as_str()),
        ("合成_資金計画.txt", "text/plain")
    );
    assert_eq!(file.generation, None);
    // A file without registered content never enters a handoff.
    let command = submit_command(&w, &[&created]);
    assert_eq!(
        w.clone().apply(VerifiedActor::Sales01, &command, T1),
        Err(WorkError::HandoffNotReady)
    );
    // The generation identity is the content-write operation, sized and hashed by the server.
    let mut mismatched = write_command(&w, &created, 12, HASH_A);
    if let Command::WriteArtifactContent { generation, .. } = &mut mismatched {
        generation.id = Uuid::now_v7();
    }
    for (command, error) in [
        (mismatched, WorkError::ValidationFailed),
        (
            write_command(&w, &created, 0, HASH_A),
            WorkError::ValidationFailed,
        ),
        (
            write_command(&w, &created, MAX_FILE_BYTES + 1, HASH_A),
            WorkError::ValidationFailed,
        ),
        (
            write_command(&w, &created, 12, "ABC"),
            WorkError::ValidationFailed,
        ),
        (
            write_command(&w, &created, 12, &HASH_A.to_uppercase()),
            WorkError::ValidationFailed,
        ),
    ] {
        assert_eq!(
            w.clone().apply(VerifiedActor::Sales01, &command, T1),
            Err(error)
        );
    }
    let stale = write_command(
        &w,
        &WorkingArtifact {
            revision: 3,
            ..created.clone()
        },
        12,
        HASH_A,
    );
    assert_eq!(
        w.clone().apply(VerifiedActor::Sales01, &stale, T1),
        Err(WorkError::RevisionConflict)
    );
    let written = write(&mut w, &created, MAX_FILE_BYTES, HASH_A);
    assert_eq!(written.revision, 1);
    let first = generation(&written);
    assert_eq!(
        (
            first.size_bytes,
            first.sha256.as_str(),
            first.stored_at.as_str(),
            first.provider_id.as_str()
        ),
        (MAX_FILE_BYTES, HASH_A, T1, WORK_ARTIFACT_PROVIDER_ID)
    );
    // Re-registering content makes a new generation; the old one is never edited.
    let rewritten = write(&mut w, &written, 7, HASH_B);
    let second = generation(&rewritten);
    assert_ne!(second.id, first.id);
    assert_eq!(rewritten.revision, 2);
    // Without a server receipt for the selected generation, submit stops.
    let command = submit_command(&w, &[&rewritten]);
    assert_eq!(
        w.clone().apply(VerifiedActor::Sales01, &command, T1),
        Err(WorkError::WorkArtifactUnavailable)
    );
    let mut stale_receipt = w.clone();
    verified(&mut stale_receipt, &[first.id]);
    assert_eq!(
        stale_receipt.apply(VerifiedActor::Sales01, &command, T1),
        Err(WorkError::WorkArtifactUnavailable)
    );
    verified(&mut w, &[second.id]);
    let MutationResult::Submitted {
        snapshot,
        next_task,
        ..
    } = w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    let pinned = &snapshot.artifacts[0];
    assert_eq!((pinned.artifact_id, pinned.revision), (rewritten.id, 2));
    assert_eq!(pinned.value, None);
    assert_eq!(
        pinned.file.as_ref().unwrap().generation,
        Some(second.clone())
    );
    // The submitter's draft stays private to its attempt; the next step reads only the pinned member.
    assert_eq!(
        w.artifact(VerifiedActor::Office01, rewritten.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        w.snapshot_file(VerifiedActor::Office01, snapshot.id, rewritten.id),
        Err(WorkError::WorkArtifactNotFound),
        "an eligible-only processor has no handoff membership before claiming"
    );
    claim(
        &mut w,
        VerifiedActor::Office01,
        OFFICE_ASSIGNMENT_ID,
        next_task.id,
    );
    assert_eq!(
        w.snapshot_file(VerifiedActor::Office01, snapshot.id, rewritten.id)
            .unwrap()
            .generation,
        Some(second)
    );
    assert_eq!(
        w.snapshot_file(VerifiedActor::Office01, snapshot.id, Uuid::now_v7()),
        Err(WorkError::WorkArtifactNotFound)
    );
    // The completed source attempt cannot be edited further.
    let late = write_command(&w, &rewritten, 3, HASH_A);
    assert!(w.clone().apply(VerifiedActor::Sales01, &late, T1).is_err());
}

#[test]
fn file_names_and_media_types_are_bounded_labels_never_local_paths() {
    let policy = OrganizationPolicy::synthetic();
    let w = sales_a(&policy);
    let attempt = |name: &str, media: &str| {
        w.clone().apply(
            VerifiedActor::Sales01,
            &Command::CreateFileArtifact {
                task_id: w.source.id,
                context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
                file_name: name.into(),
                media_type: media.into(),
            },
            T0,
        )
    };
    for name in [
        "",
        ".",
        "..",
        "../資料.txt",
        "フォルダ/資料.txt",
        "C:\\Users\\synthetic\\資料.txt",
        "改行\n.txt",
        "タブ\t.txt",
        "請求書\u{202E}fdp.exe",
        "見積\u{200F}.txt",
        "合成\u{2066}資料.txt",
        "\u{FEFF}資料.txt",
        &"あ".repeat(86),
    ] {
        assert_eq!(
            attempt(name, "text/plain"),
            Err(WorkError::ValidationFailed),
            "{name:?}"
        );
    }
    for media in [
        "",
        "text",
        "text/",
        "/plain",
        "text/plain extra",
        "テキスト/plain",
        &format!("a/{}", "b".repeat(126)),
    ] {
        assert_eq!(
            attempt("資料.txt", media),
            Err(WorkError::ValidationFailed),
            "{media:?}"
        );
    }
    assert!(
        attempt(
            &"あ".repeat(85),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        )
        .is_ok()
    );
    assert!(attempt("合成 資料(1).pdf", "application/pdf").is_ok());
}

#[test]
fn only_the_current_assignee_creates_writes_or_reads_files_and_reassignment_revokes_access() {
    let mut policy = OrganizationPolicy::synthetic();
    // A second synthetic sales holder so the source step can be reassigned.
    let mut second = policy
        .role_assignments
        .iter()
        .find(|value| value.id == SALES_ASSIGNMENT_ID)
        .unwrap()
        .clone();
    second.id = Uuid::now_v7();
    second.principal = VerifiedActor::Delegate01;
    let second_id = second.id;
    policy.role_assignments.push(second);
    let mut w = sales_a(&policy);
    let created = create(&mut w, "合成_見積.txt");
    let written = write(&mut w, &created, 10, HASH_A);
    assert!(w.artifact_file(VerifiedActor::Sales01, written.id).is_ok());
    for actor in [
        VerifiedActor::Office01,
        VerifiedActor::Approver01,
        VerifiedActor::Delegate01,
    ] {
        assert_eq!(
            w.artifact_file(actor, written.id),
            Err(WorkError::WorkArtifactNotFound),
            "{actor:?}"
        );
        let command = Command::CreateFileArtifact {
            task_id: w.source.id,
            context: ctx(actor.assignment_id(), w.source.revision),
            file_name: "合成.txt".into(),
            media_type: "text/plain".into(),
        };
        assert!(w.clone().apply(actor, &command, T0).is_err(), "{actor:?}");
    }
    // A file with no registered content has nothing to read.
    let empty = create(&mut w, "未登録.txt");
    assert_eq!(
        w.artifact_file(VerifiedActor::Sales01, empty.id),
        Err(WorkError::HandoffNotReady)
    );
    w.apply(
        VerifiedActor::Approver01,
        &Command::Assign {
            task_id: w.source.id,
            context: ctx(APPROVER_MANAGEMENT_ASSIGNMENT_ID, w.source.revision),
            expected_attempt_id: w.source.attempt_id,
            assignee: VerifiedActor::Delegate01,
            assignee_responsibility_id: second_id,
            reason: "合成の担当変更".into(),
        },
        T1,
    )
    .unwrap();
    // The new assignee takes over the attempt's private files; the old one loses them.
    assert_eq!(
        w.artifact_file(VerifiedActor::Sales01, written.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    assert_eq!(
        w.artifact_file(VerifiedActor::Delegate01, written.id)
            .unwrap()
            .generation,
        written.file.clone().unwrap().generation
    );
}

#[test]
fn discarding_removes_only_an_unsubmitted_record_and_text_drafts_keep_their_shape() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = sales_a(&policy);
    let memo = text(&mut w, "合成の非公開メモ");
    // The existing text draft JSON is unchanged: no file or provenance keys.
    let json = serde_json::to_value(&memo).unwrap();
    assert_eq!(
        json.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        [
            "attemptId",
            "id",
            "revision",
            "schemaId",
            "taskId",
            "value",
            "visibility"
        ]
        .map(String::from)
        .into()
    );
    let file = create(&mut w, "外す資料.txt");
    // A text save cannot target a file record.
    let overwrite = Command::SaveDraft {
        task_id: w.source.id,
        artifact_id: Some(file.id),
        context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
        value: TextValue {
            text: "上書き".into(),
        },
    };
    assert_eq!(
        w.clone().apply(VerifiedActor::Sales01, &overwrite, T0),
        Err(WorkError::ValidationFailed)
    );
    let discard =
        |w: &Workflow, artifact: &WorkingArtifact, revision: i64| Command::DiscardArtifact {
            task_id: w.source.id,
            artifact_id: artifact.id,
            context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
            expected_artifact_revision: revision,
        };
    assert_eq!(
        w.clone()
            .apply(VerifiedActor::Sales01, &discard(&w, &file, 1), T0),
        Err(WorkError::RevisionConflict)
    );
    let command = discard(&w, &file, 0);
    let MutationResult::ArtifactDiscarded { artifact_id, .. } =
        w.apply(VerifiedActor::Sales01, &command, T0).unwrap()
    else {
        panic!()
    };
    assert_eq!(artifact_id, file.id);
    assert_eq!(
        w.artifact(VerifiedActor::Sales01, file.id),
        Err(WorkError::WorkArtifactNotFound)
    );
    // A text-only submission keeps the existing pinned JSON shape.
    let command = submit_command(&w, &[&memo]);
    let MutationResult::Submitted { snapshot, .. } =
        w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        serde_json::to_value(&snapshot.artifacts[0]).unwrap(),
        serde_json::json!({"artifactId":memo.id,"revision":0,"schemaId":TEXT_SCHEMA_ID,"value":{"text":"合成の非公開メモ"}})
    );
    // Submitted membership can no longer be discarded.
    assert!(
        w.clone()
            .apply(VerifiedActor::Sales01, &discard(&w, &memo, 0), T1)
            .is_err()
    );
}

#[test]
fn a_returned_attempt_explicitly_imports_the_prior_submission_and_resubmits_without_changing_it() {
    let policy = OrganizationPolicy::synthetic();
    let mut w = context_b(&policy);
    claim(
        &mut w,
        VerifiedActor::Sales01,
        SALES_ASSIGNMENT_ID,
        CONTEXT_B_SALES_TASK_ID,
    );
    let memo = text(&mut w, "合成の初回メモ");
    let created = create(&mut w, "合成_資金使途.txt");
    let file = write(&mut w, &created, 20, HASH_A);
    verified(&mut w, &[generation(&file).id]);
    let command = submit_command(&w, &[&memo, &file]);
    let MutationResult::Submitted {
        snapshot: first, ..
    } = w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    let frozen = serde_json::to_value(&first).unwrap();
    claim(
        &mut w,
        VerifiedActor::Review01,
        REVIEW_ASSIGNMENT_ID,
        CONTEXT_B_REVIEW_TASK_ID,
    );
    // Import is only for an attempt created by a return.
    let import = |w: &Workflow, snapshot_id: Uuid| Command::ImportSubmission {
        task_id: CONTEXT_B_SALES_TASK_ID,
        context: ctx(SALES_ASSIGNMENT_ID, w.source.revision),
        expected_attempt_id: w.source.attempt_id,
        snapshot_id,
    };
    let review = w.next.clone().unwrap();
    let transition = w
        .detail(VerifiedActor::Review01, review.id)
        .unwrap()
        .task
        .return_transition
        .unwrap();
    w.apply(
        VerifiedActor::Review01,
        &Command::Return {
            task_id: review.id,
            context: ctx(REVIEW_ASSIGNMENT_ID, review.revision),
            expected_attempt_id: review.attempt_id,
            previous_submission_id: transition.previous_submission_id,
            target_task_id: transition.target_task_id,
            transition_id: transition.transition_id,
            reason: "合成の差戻理由".into(),
        },
        T1,
    )
    .unwrap();
    // The new attempt starts with no private draft and must be claimed first.
    assert!(
        w.artifacts
            .iter()
            .all(|value| value.attempt_id != w.source.attempt_id)
    );
    assert!(
        w.clone()
            .apply(VerifiedActor::Sales01, &import(&w, first.id), T1)
            .is_err()
    );
    claim(
        &mut w,
        VerifiedActor::Sales01,
        SALES_ASSIGNMENT_ID,
        CONTEXT_B_SALES_TASK_ID,
    );
    assert_eq!(
        w.clone()
            .apply(VerifiedActor::Sales01, &import(&w, Uuid::now_v7()), T1),
        Err(WorkError::WorkArtifactNotFound)
    );
    let command = import(&w, first.id);
    let MutationResult::SubmissionImported { artifacts, .. } =
        w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    assert_eq!(artifacts.len(), 2);
    for (imported, original) in artifacts.iter().zip([&memo, &file]) {
        assert_ne!(imported.id, original.id);
        assert_eq!(imported.attempt_id, w.source.attempt_id);
        assert_eq!(imported.revision, 0);
        assert_eq!(imported.visibility, "work_item_private");
        assert_eq!(
            imported.derived_from,
            Some(DerivedFrom {
                snapshot_id: first.id,
                artifact_id: original.id
            })
        );
    }
    assert_eq!(artifacts[0].value, memo.value);
    // The file is referenced, not copied: the same immutable generation.
    assert_eq!(generation(&artifacts[1]), generation(&file));
    assert_eq!(
        w.clone()
            .apply(VerifiedActor::Sales01, &import(&w, first.id), T1),
        Err(WorkError::ValidationFailed),
        "the same submission is imported once"
    );
    // Rework: new content for the imported file, then a fresh submission.
    let reworked = write(&mut w, &artifacts[1], 30, HASH_B);
    verified(&mut w, &[generation(&reworked).id]);
    let command = submit_command(&w, &[&artifacts[0], &reworked]);
    let MutationResult::Submitted {
        snapshot: second,
        next_task,
        ..
    } = w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    assert_eq!(second.previous_submission_id, Some(first.id));
    assert_eq!(next_task.attempt_number, 2);
    // The earlier submission and its pinned generation are byte-for-byte unchanged.
    assert_eq!(
        serde_json::to_value(
            w.snapshots
                .iter()
                .find(|value| value.id == first.id)
                .unwrap()
        )
        .unwrap(),
        frozen
    );
    claim(
        &mut w,
        VerifiedActor::Review01,
        REVIEW_ASSIGNMENT_ID,
        CONTEXT_B_REVIEW_TASK_ID,
    );
    assert_eq!(
        w.snapshot_file(VerifiedActor::Review01, second.id, reworked.id)
            .unwrap()
            .generation,
        Some(generation(&reworked))
    );
    // The reviewer still compares with the predecessor's pinned file.
    assert_eq!(
        w.snapshot_file(VerifiedActor::Review01, first.id, file.id)
            .unwrap()
            .generation,
        Some(generation(&file))
    );
    // A second return: the new attempt imports only the submission it refers to,
    // never the still-readable predecessor.
    let review = w.next.clone().unwrap();
    let transition = w
        .detail(VerifiedActor::Review01, review.id)
        .unwrap()
        .task
        .return_transition
        .unwrap();
    w.apply(
        VerifiedActor::Review01,
        &Command::Return {
            task_id: review.id,
            context: ctx(REVIEW_ASSIGNMENT_ID, review.revision),
            expected_attempt_id: review.attempt_id,
            previous_submission_id: transition.previous_submission_id,
            target_task_id: transition.target_task_id,
            transition_id: transition.transition_id,
            reason: "合成の再差戻理由".into(),
        },
        T1,
    )
    .unwrap();
    claim(
        &mut w,
        VerifiedActor::Sales01,
        SALES_ASSIGNMENT_ID,
        CONTEXT_B_SALES_TASK_ID,
    );
    assert!(w.snapshot(VerifiedActor::Sales01, first.id).is_ok());
    assert_eq!(
        w.clone()
            .apply(VerifiedActor::Sales01, &import(&w, first.id), T1),
        Err(WorkError::WorkArtifactNotFound)
    );
    let command = import(&w, second.id);
    let MutationResult::SubmissionImported { artifacts, .. } =
        w.apply(VerifiedActor::Sales01, &command, T1).unwrap()
    else {
        panic!()
    };
    assert_eq!(generation(&artifacts[1]), generation(&reworked));
}
