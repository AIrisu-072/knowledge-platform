use document_domain::{AuditEventId, DocumentId, DocumentVersionId, EventId, PrincipalRef};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApplicationError, PreparedManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionOperationId(Uuid);

impl VersionOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() != 7 {
            return Err(ApplicationError::Validation(
                "version operation id must be UUIDv7".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOperationKind {
    Create,
    Update,
    Rebase,
}

impl VersionOperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "CREATE",
            Self::Update => "UPDATE",
            Self::Rebase => "REBASE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCommandIdentity {
    operation_id: VersionOperationId,
    kind: VersionOperationKind,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
    command_digest: [u8; 32],
}

impl VersionCommandIdentity {
    pub fn new(
        operation_id: VersionOperationId,
        kind: VersionOperationKind,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        expected_revision: i64,
        actor: PrincipalRef,
        prepared: Option<&PreparedManifest>,
    ) -> Result<Self, ApplicationError> {
        if expected_revision < 0 || (kind == VersionOperationKind::Rebase) != prepared.is_none() {
            return Err(ApplicationError::Validation(
                "invalid revision or mutation manifest".to_owned(),
            ));
        }
        let command_digest = digest_command(
            kind,
            document_id,
            target_version_id,
            expected_revision,
            &actor,
            prepared,
        );
        Ok(Self {
            operation_id,
            kind,
            document_id,
            target_version_id,
            expected_revision,
            actor,
            command_digest,
        })
    }

    pub fn from_persisted(
        operation_id: VersionOperationId,
        kind: VersionOperationKind,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        expected_revision: i64,
        actor: PrincipalRef,
        command_digest: [u8; 32],
    ) -> Self {
        Self {
            operation_id,
            kind,
            document_id,
            target_version_id,
            expected_revision,
            actor,
            command_digest,
        }
    }

    pub const fn operation_id(&self) -> VersionOperationId {
        self.operation_id
    }
    pub const fn kind(&self) -> VersionOperationKind {
        self.kind
    }
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
    pub const fn target_version_id(&self) -> DocumentVersionId {
        self.target_version_id
    }
    pub const fn expected_revision(&self) -> i64 {
        self.expected_revision
    }
    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }
    pub const fn command_digest(&self) -> [u8; 32] {
        self.command_digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOperationResult {
    operation_id: VersionOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    version_no: i64,
    base_version_id: DocumentVersionId,
    resulting_revision: i64,
}

impl VersionOperationResult {
    pub fn from_persisted(
        operation_id: VersionOperationId,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        version_no: i64,
        base_version_id: DocumentVersionId,
        resulting_revision: i64,
    ) -> Self {
        Self {
            operation_id,
            document_id,
            target_version_id,
            version_no,
            base_version_id,
            resulting_revision,
        }
    }
    pub const fn operation_id(&self) -> VersionOperationId {
        self.operation_id
    }
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
    pub const fn target_version_id(&self) -> DocumentVersionId {
        self.target_version_id
    }
    pub const fn version_no(&self) -> i64 {
        self.version_no
    }
    pub const fn base_version_id(&self) -> DocumentVersionId {
        self.base_version_id
    }
    pub const fn resulting_revision(&self) -> i64 {
        self.resulting_revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOperationRecord {
    identity: VersionCommandIdentity,
    result: VersionOperationResult,
}

impl VersionOperationRecord {
    pub fn new(identity: VersionCommandIdentity, result: VersionOperationResult) -> Self {
        Self { identity, result }
    }
    pub fn identity(&self) -> &VersionCommandIdentity {
        &self.identity
    }
    pub fn result(&self) -> &VersionOperationResult {
        &self.result
    }
    pub fn matches_identity(&self, identity: &VersionCommandIdentity) -> bool {
        &self.identity == identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionMutationRecord {
    identity: VersionCommandIdentity,
    prepared: Option<PreparedManifest>,
    expected_current_version_id: DocumentVersionId,
    expected_current_manifest_digest: [u8; 32],
    domain_event_id: EventId,
    audit_event_id: AuditEventId,
    occurred_at: OffsetDateTime,
}

impl VersionMutationRecord {
    pub fn new(
        identity: VersionCommandIdentity,
        prepared: Option<PreparedManifest>,
        expected_current_version_id: DocumentVersionId,
        expected_current_manifest_digest: [u8; 32],
        domain_event_id: EventId,
        audit_event_id: AuditEventId,
        occurred_at: OffsetDateTime,
    ) -> Self {
        Self {
            identity,
            prepared,
            expected_current_version_id,
            expected_current_manifest_digest,
            domain_event_id,
            audit_event_id,
            occurred_at,
        }
    }
    pub fn identity(&self) -> &VersionCommandIdentity {
        &self.identity
    }
    pub fn prepared(&self) -> Option<&PreparedManifest> {
        self.prepared.as_ref()
    }
    pub const fn expected_current_version_id(&self) -> DocumentVersionId {
        self.expected_current_version_id
    }
    pub const fn expected_current_manifest_digest(&self) -> [u8; 32] {
        self.expected_current_manifest_digest
    }
    pub const fn domain_event_id(&self) -> EventId {
        self.domain_event_id
    }
    pub const fn audit_event_id(&self) -> AuditEventId {
        self.audit_event_id
    }
    pub const fn occurred_at(&self) -> OffsetDateTime {
        self.occurred_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateVersionCommand {
    operation_id: VersionOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateWorkingVersionCommand {
    operation_id: VersionOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseWorkingVersionCommand {
    operation_id: VersionOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
}

macro_rules! command_impl {
    ($name:ident) => {
        impl $name {
            pub fn new(
                operation_id: VersionOperationId,
                document_id: DocumentId,
                target_version_id: DocumentVersionId,
                expected_revision: i64,
                actor: PrincipalRef,
            ) -> Result<Self, ApplicationError> {
                if expected_revision < 0 {
                    return Err(ApplicationError::Validation(
                        "expected document revision cannot be negative".to_owned(),
                    ));
                }
                Ok(Self {
                    operation_id,
                    document_id,
                    target_version_id,
                    expected_revision,
                    actor,
                })
            }
            pub const fn operation_id(&self) -> VersionOperationId {
                self.operation_id
            }
            pub const fn document_id(&self) -> DocumentId {
                self.document_id
            }
            pub const fn target_version_id(&self) -> DocumentVersionId {
                self.target_version_id
            }
            pub const fn expected_revision(&self) -> i64 {
                self.expected_revision
            }
            pub const fn actor(&self) -> &PrincipalRef {
                &self.actor
            }
        }
    };
}
command_impl!(CreateVersionCommand);
command_impl!(UpdateWorkingVersionCommand);
command_impl!(RebaseWorkingVersionCommand);

fn digest_command(
    kind: VersionOperationKind,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: &PrincipalRef,
    prepared: Option<&PreparedManifest>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"document-version-operation-v0\0");
    digest_field(&mut hash, kind.as_str().as_bytes());
    hash.update(document_id.as_uuid().as_bytes());
    hash.update(target_version_id.as_uuid().as_bytes());
    hash.update(expected_revision.to_be_bytes());
    digest_field(&mut hash, actor.identity_provider().as_bytes());
    digest_field(&mut hash, actor.principal_id().as_bytes());
    if let Some(prepared) = prepared {
        hash.update(prepared.identity_digest());
        for item in prepared.items() {
            hash.update(item.file().file_id().as_uuid().as_bytes());
            hash.update(item.file().content_hash().as_bytes());
            hash.update(item.file().size_bytes().get().to_be_bytes());
            digest_field(&mut hash, item.file().media_type().as_str().as_bytes());
            digest_field(&mut hash, item.file().storage_key().as_str().as_bytes());
            digest_field(&mut hash, item.original_filename().as_bytes());
            hash.update((item.renditions().len() as u32).to_be_bytes());
            for rendition in item.renditions() {
                let file = rendition.file();
                hash.update(file.file_id().as_uuid().as_bytes());
                hash.update(file.content_hash().as_bytes());
                hash.update(file.size_bytes().get().to_be_bytes());
                digest_field(&mut hash, file.media_type().as_str().as_bytes());
                digest_field(&mut hash, file.storage_key().as_str().as_bytes());
                digest_field(&mut hash, rendition.original_filename().as_bytes());
            }
        }
    }
    hash.finalize().into()
}

fn digest_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u32).to_be_bytes());
    hash.update(bytes);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawVersionCommand {
    operation_id: VersionOperationId,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
    expected_revision: i64,
    actor: PrincipalRef,
    reason: String,
}

impl WithdrawVersionCommand {
    pub fn new(
        operation_id: VersionOperationId,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
        expected_revision: i64,
        actor: PrincipalRef,
        reason: impl Into<String>,
    ) -> Result<Self, ApplicationError> {
        let reason = reason.into();
        let reason = reason.trim();
        if expected_revision < 0 || reason.is_empty() {
            return Err(ApplicationError::Validation(
                "withdrawal requires a reason and nonnegative revision".to_owned(),
            ));
        }
        Ok(Self {
            operation_id,
            document_id,
            target_version_id,
            expected_revision,
            actor,
            reason: reason.to_owned(),
        })
    }
    pub const fn operation_id(&self) -> VersionOperationId {
        self.operation_id
    }
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }
    pub const fn target_version_id(&self) -> DocumentVersionId {
        self.target_version_id
    }
    pub const fn expected_revision(&self) -> i64 {
        self.expected_revision
    }
    pub const fn actor(&self) -> &PrincipalRef {
        &self.actor
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
    pub fn command_digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"document-version-withdraw-v0\0");
        hash.update(self.document_id.as_uuid().as_bytes());
        hash.update(self.target_version_id.as_uuid().as_bytes());
        hash.update(self.expected_revision.to_be_bytes());
        digest_field(&mut hash, self.actor.identity_provider().as_bytes());
        digest_field(&mut hash, self.actor.principal_id().as_bytes());
        digest_field(&mut hash, self.reason.as_bytes());
        hash.finalize().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawVersionResult {
    pub operation_id: VersionOperationId,
    pub document_id: DocumentId,
    pub target_version_id: DocumentVersionId,
    pub former_current_version_id: Option<DocumentVersionId>,
    pub resulting_current_version_id: Option<DocumentVersionId>,
    pub resulting_revision: i64,
    pub restoration_withheld_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawOperationRecord {
    pub command_digest: [u8; 32],
    pub result: WithdrawVersionResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawVersionRecord {
    pub command: WithdrawVersionCommand,
    pub eligible_base_id: Option<DocumentVersionId>,
    pub eligible_base_manifest_digest: Option<[u8; 32]>,
    pub restoration_withheld_reason: Option<String>,
    pub domain_event_id: EventId,
    pub audit_event_id: AuditEventId,
    pub withdrawn_at: OffsetDateTime,
}
