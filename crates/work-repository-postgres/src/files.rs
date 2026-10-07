//! Private work files (U3): bytes go to the injected Work artifact store outside
//! any row lock; the aggregate, ledger and staging record only server-computed
//! identities. Reads re-check current authority after the store read.
use super::*;
use std::collections::BTreeSet;
use work_application::{WorkArtifactStore, content_identity};

impl PostgresWorkRepository {
    /// The Work-owned shared artifact store (a separate namespace).
    pub fn with_artifact_store(mut self, store: Arc<dyn WorkArtifactStore>) -> Self {
        self.artifact_store = Some(store);
        self
    }
    fn store(&self) -> Result<&Arc<dyn WorkArtifactStore>, WorkError> {
        self.artifact_store
            .as_ref()
            .ok_or(WorkError::WorkArtifactUnavailable)
    }
    /// Server receipts for every file generation a submission would pin. A
    /// generation that cannot be confirmed in time is simply not verified; the
    /// aggregate then refuses the submission under its lock.
    pub(crate) async fn verify_generations(&self, preview: &MutationResult) -> BTreeSet<Uuid> {
        let MutationResult::Submitted { snapshot, .. } = preview else {
            return BTreeSet::new();
        };
        let Ok(store) = self.store() else {
            return BTreeSet::new();
        };
        let started = Instant::now();
        let mut verified = BTreeSet::new();
        for generation in snapshot
            .artifacts
            .iter()
            .filter_map(|value| value.file.as_ref()?.generation.clone())
        {
            let id = generation.id;
            if started.elapsed() <= PREFLIGHT_LIFETIME
                && store.verify(generation).await.is_ok()
                && started.elapsed() <= PREFLIGHT_LIFETIME
            {
                verified.insert(id);
            }
        }
        verified
    }
    /// Authorize the current assignee's file first, validate the command against
    /// a preview, store the bytes as the generation named by the operation, then
    /// commit. A committed identical operation replays without a new write.
    pub(crate) async fn write_content(
        &self,
        actor: VerifiedActor,
        artifact_id: Uuid,
        context: CommandContext,
        expected_artifact_revision: i64,
        bytes: Vec<u8>,
    ) -> Result<MutationResult, WorkError> {
        context.authorize(actor)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(WorkError::ValidationFailed);
        }
        let identity = content_identity(&bytes);
        // A committed operation replays (or conflicts) by its digest even when its
        // record has since been discarded; nothing is stored again.
        let committed = self.operation(actor, context.operation_id).await?;
        let replay = committed.is_some();
        let (workflow, task_id) = match committed {
            Some((workflow, principal, _, outcome)) => {
                if principal != actor.principal_id() {
                    return Err(WorkError::WorkItemNotFound);
                }
                match outcome {
                    MutationResult::ArtifactContentWritten { artifact, .. }
                        if artifact.id == artifact_id =>
                    {
                        (workflow, artifact.task_id)
                    }
                    _ => return Err(WorkError::OperationConflict),
                }
            }
            None => {
                let workflow = self
                    .load_for(actor, WorkTarget::Artifact(artifact_id))
                    .await?;
                let task_id = workflow.artifact(actor, artifact_id)?.task_id;
                (workflow, task_id)
            }
        };
        let command = Command::WriteArtifactContent {
            task_id,
            artifact_id,
            generation: GenerationInput {
                id: context.operation_id,
                size_bytes: identity.size_bytes,
                sha256: identity.sha256,
            },
            context,
            expected_artifact_revision,
        };
        if !replay {
            let timestamp = OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|_| WorkError::IntegrityViolation)?;
            workflow.clone().apply(actor, &command, &timestamp)?;
            self.store()?
                .put(command.context().operation_id, bytes)
                .await?;
        }
        Ok(self.execute_command_receipt(actor, command).await?.outcome)
    }
    pub(crate) async fn read_artifact_content(
        &self,
        actor: VerifiedActor,
        artifact_id: Uuid,
    ) -> Result<(WorkFile, Vec<u8>), WorkError> {
        let store = self.store()?;
        let workflow = self
            .load_for(actor, WorkTarget::Artifact(artifact_id))
            .await?;
        let file = workflow.artifact_file(actor, artifact_id)?;
        let generation = file.generation.clone().ok_or(WorkError::HandoffNotReady)?;
        let bytes = store.read(generation.clone()).await?;
        // Current authority again immediately before disclosure; a replaced
        // generation is a conflict, never the older bytes under the new label.
        let current = self
            .load_id(actor, workflow.id)
            .await?
            .artifact_file(actor, artifact_id)?;
        if current.generation.as_ref() != Some(&generation) {
            return Err(WorkError::RevisionConflict);
        }
        Ok((file, bytes))
    }
    pub(crate) async fn read_snapshot_content(
        &self,
        actor: VerifiedActor,
        snapshot_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<(WorkFile, Vec<u8>), WorkError> {
        let store = self.store()?;
        let workflow = self
            .load_for(actor, WorkTarget::Snapshot(snapshot_id))
            .await?;
        let file = workflow.snapshot_file(actor, snapshot_id, artifact_id)?;
        let generation = file
            .generation
            .clone()
            .ok_or(WorkError::WorkArtifactNotFound)?;
        let bytes = store.read(generation).await?;
        self.load_id(actor, workflow.id)
            .await?
            .snapshot_file(actor, snapshot_id, artifact_id)?;
        Ok((file, bytes))
    }
}
/// Staging carries identities only: never a file name or draft body.
pub(crate) fn stage_payload(payload: &mut serde_json::Value, result: &MutationResult) {
    let describe = |artifact: &WorkingArtifact| {
        let generation = artifact
            .file
            .as_ref()
            .and_then(|file| file.generation.as_ref());
        serde_json::json!({
            "artifactId": artifact.id,
            "schemaId": artifact.schema_id,
            "attemptId": artifact.attempt_id,
            "generationId": generation.map(|value| value.id),
            "sizeBytes": generation.map(|value| value.size_bytes),
            "sha256": generation.map(|value| value.sha256.clone()),
            "derivedFromSnapshotId": artifact.derived_from.as_ref().map(|value| value.snapshot_id),
        })
    };
    match result {
        MutationResult::ArtifactCreated { artifact, .. }
        | MutationResult::ArtifactContentWritten { artifact, .. } => {
            payload["artifact"] = describe(artifact);
        }
        MutationResult::ArtifactDiscarded { artifact_id, .. } => {
            payload["artifactId"] = serde_json::json!(artifact_id);
        }
        MutationResult::SubmissionImported { artifacts, .. } => {
            payload["artifacts"] = artifacts.iter().map(describe).collect();
        }
        MutationResult::Submitted { snapshot, .. } => {
            // Text-only submissions keep their existing payload shape.
            let pinned: Vec<Uuid> = snapshot
                .artifacts
                .iter()
                .filter_map(|value| Some(value.file.as_ref()?.generation.as_ref()?.id))
                .collect();
            if !pinned.is_empty() {
                payload["pinnedGenerationIds"] = serde_json::json!(pinned);
            }
        }
        _ => {}
    }
}
