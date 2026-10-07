//! Private work files (U3): Work-owned immutable generations in the shared
//! artifact store, pinned into the Handoff Snapshot on submit, and explicit
//! import of a returned submission into a new private draft.
use crate::*;
use std::{collections::BTreeSet, sync::Arc};

pub const FILE_SCHEMA_ID: &str = "organization.work-file.v1";
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_FILE_NAME_BYTES: usize = 255;
pub const MAX_MEDIA_TYPE_BYTES: usize = 127;
/// The Work-owned shared store; never a Document, ACL or local path.
pub const WORK_ARTIFACT_PROVIDER_ID: &str = "organization.work-artifacts";

/// A file's display label and its current immutable generation, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkFile {
    pub file_name: String,
    pub media_type: String,
    pub generation: Option<FileGeneration>,
}
/// One immutable stored content generation, sized and hashed by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileGeneration {
    pub id: Uuid,
    pub size_bytes: u64,
    pub sha256: String,
    pub stored_at: String,
    pub provider_id: String,
}
/// Server-computed content identity carried by a content-write command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationInput {
    pub id: Uuid,
    pub size_bytes: u64,
    pub sha256: String,
}
/// Provenance of an artifact imported from a prior submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedFrom {
    pub snapshot_id: Uuid,
    pub artifact_id: Uuid,
}
/// Non-persisted server verification of stored generations. Nothing attached
/// verifies nothing; preview defers the check to the store fan-out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum GenerationReceipts {
    #[default]
    None,
    Deferred,
    Verified(Arc<BTreeSet<Uuid>>),
}

/// A display label only: never a path, separator, control character or dot entry.
pub(crate) fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILE_NAME_BYTES
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|value| value == '/' || value == '\\' || value.is_control())
}
pub(crate) fn valid_media_type(value: &str) -> bool {
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&byte))
    };
    value.len() <= MAX_MEDIA_TYPE_BYTES
        && value
            .split_once('/')
            .is_some_and(|(kind, subtype)| token(kind) && token(subtype))
}
pub(crate) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Workflow {
    /// Attach the server's verification of stored generations for one commit.
    pub fn attach_verified_generations(&mut self, ids: BTreeSet<Uuid>) {
        self.authority.generations = GenerationReceipts::Verified(Arc::new(ids));
    }
    /// Preview only: validate everything else before the store fan-out.
    pub fn defer_generation_receipts(&mut self) {
        self.authority.generations = GenerationReceipts::Deferred;
    }
    pub(crate) fn generation_verified(&self, id: Uuid) -> bool {
        match &self.authority.generations {
            GenerationReceipts::None => false,
            GenerationReceipts::Deferred => true,
            GenerationReceipts::Verified(ids) => ids.contains(&id),
        }
    }
    /// The current generation of a private draft file, for its current assignee.
    pub fn artifact_file(&self, actor: VerifiedActor, id: Uuid) -> Result<WorkFile, WorkError> {
        let file = self
            .artifact(actor, id)?
            .file
            .ok_or(WorkError::WorkArtifactNotFound)?;
        if file.generation.is_none() {
            return Err(WorkError::HandoffNotReady);
        }
        Ok(file)
    }
    /// A pinned file of a submission, under the snapshot's own read rules.
    pub fn snapshot_file(
        &self,
        actor: VerifiedActor,
        snapshot_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<WorkFile, WorkError> {
        self.snapshot(actor, snapshot_id)?
            .artifacts
            .into_iter()
            .find(|value| value.artifact_id == artifact_id)
            .and_then(|value| value.file)
            .filter(|file| file.generation.is_some())
            .ok_or(WorkError::WorkArtifactNotFound)
    }
    /// Draft edits exist only on the active source attempt.
    fn editable_source(&self, task_id: Uuid) -> Result<(), WorkError> {
        if task_id != self.source.id || self.source.state != TaskState::Active {
            return Err(WorkError::HandoffNotReady);
        }
        Ok(())
    }
    fn attempt_artifact_count(&self) -> usize {
        self.artifacts
            .iter()
            .filter(|value| value.attempt_id == self.source.attempt_id)
            .count()
    }
    fn bump_source(&mut self) -> Result<(), WorkError> {
        self.source.revision = self
            .source
            .revision
            .checked_add(1)
            .ok_or(WorkError::IntegrityViolation)?;
        Ok(())
    }
    pub(crate) fn apply_files(
        &mut self,
        actor: VerifiedActor,
        command: &Command,
        now: &str,
    ) -> Result<MutationResult, WorkError> {
        match command {
            Command::CreateFileArtifact {
                task_id,
                file_name,
                media_type,
                ..
            } => {
                self.editable_source(*task_id)?;
                if !valid_file_name(file_name) || !valid_media_type(media_type) {
                    return Err(WorkError::ValidationFailed);
                }
                if self.attempt_artifact_count() >= MAX_ARTIFACTS {
                    return Err(WorkError::ValidationFailed);
                }
                let artifact = WorkingArtifact {
                    id: Uuid::now_v7(),
                    task_id: *task_id,
                    attempt_id: self.source.attempt_id,
                    revision: 0,
                    schema_id: FILE_SCHEMA_ID.into(),
                    value: None,
                    visibility: "work_item_private".into(),
                    file: Some(WorkFile {
                        file_name: file_name.clone(),
                        media_type: media_type.clone(),
                        generation: None,
                    }),
                    derived_from: None,
                };
                self.artifacts.push(artifact.clone());
                self.bump_source()?;
                Ok(MutationResult::ArtifactCreated {
                    task: self.summary(actor, &self.source),
                    artifact,
                })
            }
            Command::WriteArtifactContent {
                task_id,
                artifact_id,
                context,
                expected_artifact_revision,
                generation,
            } => {
                self.editable_source(*task_id)?;
                if generation.id != context.operation_id
                    || generation.size_bytes == 0
                    || generation.size_bytes > MAX_FILE_BYTES
                    || !valid_sha256(&generation.sha256)
                {
                    return Err(WorkError::ValidationFailed);
                }
                let attempt = self.source.attempt_id;
                let artifact = self
                    .artifacts
                    .iter_mut()
                    .find(|value| value.id == *artifact_id && value.attempt_id == attempt)
                    .ok_or(WorkError::WorkArtifactNotFound)?;
                if artifact.revision != *expected_artifact_revision {
                    return Err(WorkError::RevisionConflict);
                }
                // Only a file record takes stored content; a text draft never does.
                let file = artifact.file.as_mut().ok_or(WorkError::ValidationFailed)?;
                file.generation = Some(FileGeneration {
                    id: generation.id,
                    size_bytes: generation.size_bytes,
                    sha256: generation.sha256.clone(),
                    stored_at: now.into(),
                    provider_id: WORK_ARTIFACT_PROVIDER_ID.into(),
                });
                artifact.revision = artifact
                    .revision
                    .checked_add(1)
                    .ok_or(WorkError::IntegrityViolation)?;
                let artifact = artifact.clone();
                self.bump_source()?;
                Ok(MutationResult::ArtifactContentWritten {
                    task: self.summary(actor, &self.source),
                    artifact,
                })
            }
            Command::DiscardArtifact {
                task_id,
                artifact_id,
                expected_artifact_revision,
                ..
            } => {
                self.editable_source(*task_id)?;
                let attempt = self.source.attempt_id;
                let index = self
                    .artifacts
                    .iter()
                    .position(|value| value.id == *artifact_id && value.attempt_id == attempt)
                    .ok_or(WorkError::WorkArtifactNotFound)?;
                if self.artifacts[index].revision != *expected_artifact_revision {
                    return Err(WorkError::RevisionConflict);
                }
                // The record leaves the draft; stored bytes are never deleted here.
                self.artifacts.remove(index);
                self.bump_source()?;
                Ok(MutationResult::ArtifactDiscarded {
                    task: self.summary(actor, &self.source),
                    artifact_id: *artifact_id,
                })
            }
            Command::ImportSubmission {
                task_id,
                expected_attempt_id,
                snapshot_id,
                ..
            } => {
                self.editable_source(*task_id)?;
                if self.source.attempt_id != *expected_attempt_id {
                    return Err(WorkError::RevisionConflict);
                }
                if self.source.return_instruction_id.is_none() {
                    return Err(WorkError::HandoffNotReady);
                }
                // Only the submission this returned attempt refers to.
                let snapshot = self.snapshot(actor, *snapshot_id)?;
                if self.source.handoff_snapshot_id != Some(snapshot.id)
                    || snapshot.source_task_id != self.source.id
                {
                    return Err(WorkError::WorkArtifactNotFound);
                }
                let attempt = self.source.attempt_id;
                if self.artifacts.iter().any(|value| {
                    value.attempt_id == attempt
                        && value
                            .derived_from
                            .as_ref()
                            .is_some_and(|from| from.snapshot_id == snapshot.id)
                }) || self.attempt_artifact_count() + snapshot.artifacts.len() > MAX_ARTIFACTS
                {
                    return Err(WorkError::ValidationFailed);
                }
                let imported: Vec<WorkingArtifact> = snapshot
                    .artifacts
                    .iter()
                    .map(|pinned| WorkingArtifact {
                        id: Uuid::now_v7(),
                        task_id: *task_id,
                        attempt_id: attempt,
                        revision: 0,
                        schema_id: pinned.schema_id.clone(),
                        value: pinned.value.clone(),
                        visibility: "work_item_private".into(),
                        // The same immutable generation is referenced, never copied.
                        file: pinned.file.clone(),
                        derived_from: Some(DerivedFrom {
                            snapshot_id: snapshot.id,
                            artifact_id: pinned.artifact_id,
                        }),
                    })
                    .collect();
                self.artifacts.extend(imported.iter().cloned());
                self.bump_source()?;
                Ok(MutationResult::SubmissionImported {
                    task: self.summary(actor, &self.source),
                    artifacts: imported,
                })
            }
            _ => Err(WorkError::IntegrityViolation),
        }
    }
}
