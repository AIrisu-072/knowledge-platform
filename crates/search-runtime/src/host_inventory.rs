//! B5 (P7-R01P/R02): the trusted host registration inventory.
//!
//! The publisher takes one versioned operator input — every tenant of the
//! deployment (including tenants without registrations) and both namespaces'
//! server-owned configurations — validates it into registrations exactly as
//! the composition root does, and commits the immutable revision, the
//! forward-only head CAS and a `host.registration.changed` Search Audit event
//! in one PostgreSQL transaction. One namespace or one tenant is never
//! visible alone. When the commit outcome is unknown, a separate connection
//! re-reads the head before the outcome is reported.
//!
//! The read adapter is the production [`HostRegistrationSnapshotPort`]: it
//! reads the head's revision in one read-only snapshot, rebuilds every
//! registration through the same validated constructors (a Document binding
//! must still be a connected adapter) and requires the stored namespace
//! digests to equal the recomputed ones.

use std::collections::BTreeSet;
use std::sync::Arc;

use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::scoped::{RegistrationRevision, TenantId, VisibilityRevision};
use search_application::search_core::id::SourceId;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_application::source_registration::{
    CompleteDesiredRegistrations, ConnectedDocumentAdapterWitness, DocumentAdapterCapabilityPort,
    DocumentAdapterRef, DocumentSourceRegistration, HostRegistrationSnapshot,
    HostRegistrationSnapshotPort, RegistrationNamespace, RegistrationSetRevision,
    ServerDocumentRegistrationConfig, SourceRegistration,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::audit::{SearchAuditEvent, append_search_audit_on};

/// The trusted operator's whole registration input for one revision.
#[derive(Debug, Clone)]
pub struct HostRegistrationInputV1 {
    pub deployment_epoch: u64,
    pub authority_revision: u64,
    /// Every tenant of the deployment, with or without registrations.
    pub tenants: Vec<TenantId>,
    pub documents: Vec<ServerDocumentRegistrationConfig>,
    pub remotes: Vec<ServerRemoteRegistrationConfig>,
    /// Non-secret provenance of the writer (an operator or a pipeline ref).
    pub writer_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// The input is incomplete, duplicated or fails registration validation.
    Invalid(&'static str),
    /// The revision is not after the current head.
    Stale,
    /// The same revision was already published with other content.
    Conflict,
    /// The database refused before commit; nothing was published.
    Store,
    /// The commit outcome could not be proven; re-read before serving.
    Unknown,
}

impl From<sqlx::Error> for InventoryError {
    fn from(_: sqlx::Error) -> Self {
        Self::Store
    }
}

impl From<InventoryError> for SearchError {
    fn from(error: InventoryError) -> Self {
        match error {
            InventoryError::Invalid(what) => {
                SearchError::InvalidRequest(format!("host inventory: {what}"))
            }
            other => SearchError::SourceUnavailable(format!("host inventory: {other:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    Published,
    /// The head already names exactly this revision and content.
    AlreadyCurrent,
}

// ---- versioned DTO -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentConfigV1 {
    tenant: String,
    source_id: Uuid,
    document_adapter_ref: String,
    allowed_resource_kinds: Vec<ResourceKind>,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    retention_mode: RetentionMode,
    registration_revision: u64,
    visibility_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointV1 {
    scheme: String,
    host: String,
    port: u16,
    base_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsV1 {
    call_millis: u64,
    evaluation_millis: u64,
    max_request_bytes: usize,
    max_decoded_response_bytes: usize,
    max_hits_per_page: usize,
    max_pages_or_requests: usize,
    max_hits: usize,
    max_actions: usize,
    max_native_id_bytes: usize,
    max_cursor_bytes: usize,
    max_json_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum AccessContractV1 {
    PerItem,
    PublicReadWithFieldPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteConfigV1 {
    tenant: String,
    source_id: Uuid,
    provider_kind: String,
    endpoint: EndpointV1,
    supported_modes: Vec<DiscoveryMode>,
    enumeration_semantics: EnumerationSemantics,
    authority_predicates: Vec<String>,
    allowed_resource_kinds: Vec<ResourceKind>,
    current_access_contract: AccessContractV1,
    retention_mode: RetentionMode,
    freshness_policy: Option<String>,
    canonical_upstream_lineage: String,
    limits: LimitsV1,
    registration_revision: u64,
    visibility_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryDtoV1 {
    dto_version: String,
    deployment_epoch: u64,
    authority_revision: u64,
    tenants: Vec<String>,
    documents: Vec<DocumentConfigV1>,
    remotes: Vec<RemoteConfigV1>,
}

fn document_dto(config: &ServerDocumentRegistrationConfig) -> DocumentConfigV1 {
    DocumentConfigV1 {
        tenant: config.tenant.as_str().to_owned(),
        source_id: config.source_id.as_uuid(),
        document_adapter_ref: config.document_adapter_ref.as_str().to_owned(),
        allowed_resource_kinds: config.allowed_resource_kinds.clone(),
        supported_modes: config.supported_modes.clone(),
        enumeration_semantics: config.enumeration_semantics,
        retention_mode: config.retention_mode,
        registration_revision: config.registration_revision.get(),
        visibility_revision: config.visibility_revision.get(),
    }
}

fn remote_dto(config: &ServerRemoteRegistrationConfig) -> RemoteConfigV1 {
    let limits = config.limits;
    RemoteConfigV1 {
        tenant: config.tenant.as_str().to_owned(),
        source_id: config.source_id.as_uuid(),
        provider_kind: config.provider_kind.clone(),
        endpoint: EndpointV1 {
            scheme: config.endpoint.scheme().to_owned(),
            host: config.endpoint.host().to_owned(),
            port: config.endpoint.port(),
            base_path: config.endpoint.base_path().to_owned(),
        },
        supported_modes: config.supported_modes.clone(),
        enumeration_semantics: config.enumeration_semantics,
        authority_predicates: config.authority_predicates.clone(),
        allowed_resource_kinds: config.allowed_resource_kinds.clone(),
        current_access_contract: match config.current_access_contract {
            CurrentAccessContract::PerItem => AccessContractV1::PerItem,
            CurrentAccessContract::PublicReadWithFieldPolicy => {
                AccessContractV1::PublicReadWithFieldPolicy
            }
        },
        retention_mode: config.retention_mode,
        freshness_policy: config.freshness_policy.clone(),
        canonical_upstream_lineage: config.canonical_upstream_lineage.clone(),
        limits: LimitsV1 {
            call_millis: limits.call_millis,
            evaluation_millis: limits.evaluation_millis,
            max_request_bytes: limits.max_request_bytes,
            max_decoded_response_bytes: limits.max_decoded_response_bytes,
            max_hits_per_page: limits.max_hits_per_page,
            max_pages_or_requests: limits.max_pages_or_requests,
            max_hits: limits.max_hits,
            max_actions: limits.max_actions,
            max_native_id_bytes: limits.max_native_id_bytes,
            max_cursor_bytes: limits.max_cursor_bytes,
            max_json_depth: limits.max_json_depth,
        },
        registration_revision: config.registration_revision.get(),
        visibility_revision: config.visibility_revision.get(),
    }
}

fn invalid(what: &'static str) -> impl FnOnce(SearchError) -> InventoryError {
    move |_| InventoryError::Invalid(what)
}

fn document_config(
    dto: &DocumentConfigV1,
) -> Result<ServerDocumentRegistrationConfig, InventoryError> {
    Ok(ServerDocumentRegistrationConfig {
        tenant: TenantId::new(dto.tenant.clone()).map_err(invalid("tenant"))?,
        source_id: SourceId::from_uuid(dto.source_id),
        document_adapter_ref: DocumentAdapterRef::new(dto.document_adapter_ref.clone())
            .map_err(invalid("document adapter ref"))?,
        allowed_resource_kinds: dto.allowed_resource_kinds.clone(),
        supported_modes: dto.supported_modes.clone(),
        enumeration_semantics: dto.enumeration_semantics,
        retention_mode: dto.retention_mode,
        registration_revision: RegistrationRevision::new(dto.registration_revision)
            .map_err(invalid("registration revision"))?,
        visibility_revision: VisibilityRevision::new(dto.visibility_revision)
            .map_err(invalid("visibility revision"))?,
    })
}

fn remote_config(dto: &RemoteConfigV1) -> Result<ServerRemoteRegistrationConfig, InventoryError> {
    let limits = &dto.limits;
    Ok(ServerRemoteRegistrationConfig {
        tenant: TenantId::new(dto.tenant.clone()).map_err(invalid("tenant"))?,
        source_id: SourceId::from_uuid(dto.source_id),
        provider_kind: dto.provider_kind.clone(),
        endpoint: RegisteredEndpoint::new(
            dto.endpoint.scheme.clone(),
            dto.endpoint.host.clone(),
            dto.endpoint.port,
            dto.endpoint.base_path.clone(),
        )
        .map_err(invalid("endpoint"))?,
        supported_modes: dto.supported_modes.clone(),
        enumeration_semantics: dto.enumeration_semantics,
        authority_predicates: dto.authority_predicates.clone(),
        allowed_resource_kinds: dto.allowed_resource_kinds.clone(),
        current_access_contract: match dto.current_access_contract {
            AccessContractV1::PerItem => CurrentAccessContract::PerItem,
            AccessContractV1::PublicReadWithFieldPolicy => {
                CurrentAccessContract::PublicReadWithFieldPolicy
            }
        },
        retention_mode: dto.retention_mode,
        freshness_policy: dto.freshness_policy.clone(),
        canonical_upstream_lineage: dto.canonical_upstream_lineage.clone(),
        limits: RemoteRegistrationLimits {
            call_millis: limits.call_millis,
            evaluation_millis: limits.evaluation_millis,
            max_request_bytes: limits.max_request_bytes,
            max_decoded_response_bytes: limits.max_decoded_response_bytes,
            max_hits_per_page: limits.max_hits_per_page,
            max_pages_or_requests: limits.max_pages_or_requests,
            max_hits: limits.max_hits,
            max_actions: limits.max_actions,
            max_native_id_bytes: limits.max_native_id_bytes,
            max_cursor_bytes: limits.max_cursor_bytes,
            max_json_depth: limits.max_json_depth,
        },
        registration_revision: RegistrationRevision::new(dto.registration_revision)
            .map_err(invalid("registration revision"))?,
        visibility_revision: VisibilityRevision::new(dto.visibility_revision)
            .map_err(invalid("visibility revision"))?,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn set_digest_hex(snapshot: &CompleteDesiredRegistrations) -> String {
    let hex: String = snapshot
        .set_digest()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// One validated revision: both complete namespaces and the canonical DTO.
struct ValidatedInventory {
    epoch: i64,
    revision: i64,
    dto: InventoryDtoV1,
    documents: CompleteDesiredRegistrations,
    remotes: CompleteDesiredRegistrations,
    roster_digest: String,
    inventory_digest: String,
}

/// A captured namespace, served back through the host snapshot port.
struct Captured(HostRegistrationSnapshot);

impl HostRegistrationSnapshotPort for Captured {
    fn snapshot<'a>(
        &'a self,
        _namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

async fn validate(
    dto: InventoryDtoV1,
    adapters: &dyn DocumentAdapterCapabilityPort,
) -> Result<ValidatedInventory, InventoryError> {
    if dto.dto_version != "v1" {
        return Err(InventoryError::Invalid("dto version"));
    }
    let epoch = i64::try_from(dto.deployment_epoch)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(InventoryError::Invalid("deployment epoch"))?;
    let revision = i64::try_from(dto.authority_revision)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(InventoryError::Invalid("authority revision"))?;
    let roster: BTreeSet<&str> = dto.tenants.iter().map(String::as_str).collect();
    if roster.is_empty() || roster.len() != dto.tenants.len() {
        return Err(InventoryError::Invalid("tenant roster"));
    }
    for tenant in &dto.tenants {
        TenantId::new(tenant.clone()).map_err(invalid("tenant"))?;
    }
    let mut ids = BTreeSet::new();
    let mut documents = Vec::new();
    for entry in &dto.documents {
        if !roster.contains(entry.tenant.as_str()) || !ids.insert(entry.source_id) {
            return Err(InventoryError::Invalid("document tenant or SourceId"));
        }
        let config = document_config(entry)?;
        let witness = ConnectedDocumentAdapterWitness::from_connected_port(
            adapters,
            &config.document_adapter_ref,
        )
        .await
        .map_err(|_| InventoryError::Store)?
        .ok_or(InventoryError::Invalid("Document adapter is not connected"))?;
        documents.push(SourceRegistration::Document(
            DocumentSourceRegistration::from_server_config(config, &witness)
                .map_err(invalid("Document registration"))?,
        ));
    }
    let mut remotes = Vec::new();
    for entry in &dto.remotes {
        if !roster.contains(entry.tenant.as_str()) || !ids.insert(entry.source_id) {
            return Err(InventoryError::Invalid("remote tenant or SourceId"));
        }
        remotes.push(SourceRegistration::Remote(
            RemoteSourceRegistration::from_server_config(remote_config(entry)?)
                .map_err(invalid("remote registration"))?,
        ));
    }
    let set_revision = RegistrationSetRevision::new(dto.authority_revision)
        .map_err(invalid("authority revision"))?;
    let capture = |namespace, registrations| async move {
        let snapshot = HostRegistrationSnapshot::from_complete_host_inventory(
            namespace,
            set_revision,
            registrations,
        )
        .map_err(invalid("namespace"))?;
        CompleteDesiredRegistrations::capture(&Captured(snapshot), namespace)
            .await
            .map_err(invalid("namespace digest"))
    };
    let documents = capture(RegistrationNamespace::Document, documents).await?;
    let remotes = capture(RegistrationNamespace::Remote, remotes).await?;
    let roster_text =
        serde_json::to_vec(&dto.tenants).map_err(|_| InventoryError::Invalid("roster"))?;
    let canonical = serde_json::to_vec(&dto).map_err(|_| InventoryError::Invalid("dto"))?;
    Ok(ValidatedInventory {
        epoch,
        revision,
        roster_digest: sha256_hex(&roster_text),
        inventory_digest: sha256_hex(&canonical),
        dto,
        documents,
        remotes,
    })
}

fn canonical_dto(input: &HostRegistrationInputV1) -> InventoryDtoV1 {
    let mut tenants: Vec<String> = input
        .tenants
        .iter()
        .map(|tenant| tenant.as_str().to_owned())
        .collect();
    tenants.sort();
    let mut documents: Vec<DocumentConfigV1> = input.documents.iter().map(document_dto).collect();
    documents.sort_by_key(|entry| entry.source_id);
    let mut remotes: Vec<RemoteConfigV1> = input.remotes.iter().map(remote_dto).collect();
    remotes.sort_by_key(|entry| entry.source_id);
    InventoryDtoV1 {
        dto_version: "v1".into(),
        deployment_epoch: input.deployment_epoch,
        authority_revision: input.authority_revision,
        tenants,
        documents,
        remotes,
    }
}

// ---- publisher -------------------------------------------------------------

/// P7-R01P: the only writer of the host registration inventory.
pub struct PgHostInventoryPublisher {
    pool: PgPool,
    adapters: Arc<dyn DocumentAdapterCapabilityPort>,
}

impl PgHostInventoryPublisher {
    pub fn new(pool: PgPool, adapters: Arc<dyn DocumentAdapterCapabilityPort>) -> Self {
        Self { pool, adapters }
    }

    pub async fn publish(
        &self,
        input: &HostRegistrationInputV1,
    ) -> Result<PublishOutcome, InventoryError> {
        if !input
            .writer_ref
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:@/-".contains(&byte))
            || input.writer_ref.is_empty()
            || input.writer_ref.len() > 128
        {
            return Err(InventoryError::Invalid("writer ref"));
        }
        let inventory = validate(canonical_dto(input), self.adapters.as_ref()).await?;
        let dto =
            serde_json::to_value(&inventory.dto).map_err(|_| InventoryError::Invalid("dto"))?;
        let mut tx = self.pool.begin().await?;
        let head: Option<(i64, i64, String)> = sqlx::query_as(
            "SELECT deployment_epoch, authority_revision, inventory_digest \
             FROM search_host_inventory_head WHERE singleton FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((epoch, revision, digest)) = &head {
            let at = (inventory.epoch, inventory.revision);
            if at == (*epoch, *revision) {
                return if *digest == inventory.inventory_digest {
                    Ok(PublishOutcome::AlreadyCurrent)
                } else {
                    Err(InventoryError::Conflict)
                };
            }
            if at < (*epoch, *revision) {
                return Err(InventoryError::Stale);
            }
        }
        sqlx::query(
            "INSERT INTO search_host_inventory_revision (deployment_epoch,authority_revision, \
             schema_version,tenant_roster_digest,document_set_digest,remote_set_digest, \
             inventory_digest,tenant_count,document_count,remote_count,inventory_dto, \
             writer_ref,published_at) \
             VALUES ($1,$2,'v1',$3,$4,$5,$6,$7,$8,$9,$10,$11,clock_timestamp())",
        )
        .bind(inventory.epoch)
        .bind(inventory.revision)
        .bind(&inventory.roster_digest)
        .bind(set_digest_hex(&inventory.documents))
        .bind(set_digest_hex(&inventory.remotes))
        .bind(&inventory.inventory_digest)
        .bind(
            i32::try_from(inventory.dto.tenants.len())
                .map_err(|_| InventoryError::Invalid("tenants"))?,
        )
        .bind(
            i32::try_from(inventory.documents.len())
                .map_err(|_| InventoryError::Invalid("documents"))?,
        )
        .bind(
            i32::try_from(inventory.remotes.len())
                .map_err(|_| InventoryError::Invalid("remotes"))?,
        )
        .bind(&dto)
        .bind(&input.writer_ref)
        .execute(&mut *tx)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                InventoryError::Conflict
            }
            _ => InventoryError::Store,
        })?;
        let moved = match &head {
            None => {
                sqlx::query(
                    "INSERT INTO search_host_inventory_head (singleton,deployment_epoch, \
                 authority_revision,inventory_digest,updated_at) \
                 VALUES (TRUE,$1,$2,$3,clock_timestamp())",
                )
                .bind(inventory.epoch)
                .bind(inventory.revision)
                .bind(&inventory.inventory_digest)
                .execute(&mut *tx)
                .await?
            }
            Some((epoch, revision, _)) => {
                sqlx::query(
                    "UPDATE search_host_inventory_head SET deployment_epoch=$1, \
                 authority_revision=$2, inventory_digest=$3, updated_at=clock_timestamp() \
                 WHERE singleton AND deployment_epoch=$4 AND authority_revision=$5",
                )
                .bind(inventory.epoch)
                .bind(inventory.revision)
                .bind(&inventory.inventory_digest)
                .bind(epoch)
                .bind(revision)
                .execute(&mut *tx)
                .await?
            }
        };
        if moved.rows_affected() != 1 {
            return Err(InventoryError::Stale);
        }
        append_search_audit_on(
            &mut tx,
            &SearchAuditEvent::host_registration_changed(
                &input.writer_ref,
                &format!("epoch:{}:revision:{}", inventory.epoch, inventory.revision),
            ),
        )
        .await
        .map_err(|_| InventoryError::Store)?;
        if tx.commit().await.is_err() {
            // Prove the outcome from a fresh connection before reporting it.
            let head: Option<(i64, i64, String)> = sqlx::query_as(
                "SELECT deployment_epoch, authority_revision, inventory_digest \
                 FROM search_host_inventory_head WHERE singleton",
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| InventoryError::Unknown)?;
            return match head {
                Some((epoch, revision, digest))
                    if (epoch, revision) == (inventory.epoch, inventory.revision)
                        && digest == inventory.inventory_digest =>
                {
                    Ok(PublishOutcome::Published)
                }
                _ => Err(InventoryError::Unknown),
            };
        }
        Ok(PublishOutcome::Published)
    }
}

// ---- read adapter ----------------------------------------------------------

/// P7-R02: the production host registration snapshot port.
pub struct PgHostRegistrationSource {
    pool: PgPool,
    adapters: Arc<dyn DocumentAdapterCapabilityPort>,
}

impl PgHostRegistrationSource {
    pub fn new(pool: PgPool, adapters: Arc<dyn DocumentAdapterCapabilityPort>) -> Self {
        Self { pool, adapters }
    }

    /// Both namespaces of the current head, captured from one snapshot.
    pub async fn capture(
        &self,
    ) -> Result<(CompleteDesiredRegistrations, CompleteDesiredRegistrations), InventoryError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query(
            "SELECT r.deployment_epoch, r.authority_revision, r.inventory_digest, \
             r.tenant_roster_digest, r.document_set_digest, r.remote_set_digest, \
             r.inventory_dto FROM search_host_inventory_head h \
             JOIN search_host_inventory_revision r USING (deployment_epoch, authority_revision) \
             WHERE h.singleton AND h.inventory_digest = r.inventory_digest",
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(InventoryError::Invalid("no published host inventory"))?;
        tx.rollback().await?;
        let dto: InventoryDtoV1 = serde_json::from_value(row.try_get("inventory_dto")?)
            .map_err(|_| InventoryError::Invalid("stored dto"))?;
        if i64::try_from(dto.deployment_epoch).ok() != Some(row.try_get("deployment_epoch")?)
            || i64::try_from(dto.authority_revision).ok()
                != Some(row.try_get("authority_revision")?)
        {
            return Err(InventoryError::Invalid("stored revision"));
        }
        let inventory = validate(dto, self.adapters.as_ref()).await?;
        if inventory.inventory_digest != row.try_get::<String, _>("inventory_digest")?
            || inventory.roster_digest != row.try_get::<String, _>("tenant_roster_digest")?
            || set_digest_hex(&inventory.documents)
                != row.try_get::<String, _>("document_set_digest")?
            || set_digest_hex(&inventory.remotes)
                != row.try_get::<String, _>("remote_set_digest")?
        {
            return Err(InventoryError::Invalid("stored digest"));
        }
        Ok((inventory.documents, inventory.remotes))
    }

    /// The tenant roster of the current head, including empty tenants.
    pub async fn tenants(&self) -> Result<Vec<TenantId>, InventoryError> {
        let dto: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT r.inventory_dto FROM search_host_inventory_head h \
             JOIN search_host_inventory_revision r USING (deployment_epoch, authority_revision) \
             WHERE h.singleton",
        )
        .fetch_optional(&self.pool)
        .await?;
        let dto: InventoryDtoV1 = serde_json::from_value(
            dto.ok_or(InventoryError::Invalid("no published host inventory"))?,
        )
        .map_err(|_| InventoryError::Invalid("stored dto"))?;
        dto.tenants
            .into_iter()
            .map(|tenant| TenantId::new(tenant).map_err(invalid("tenant")))
            .collect()
    }
}

impl HostRegistrationSnapshotPort for PgHostRegistrationSource {
    fn snapshot<'a>(
        &'a self,
        namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot> {
        Box::pin(async move {
            let (documents, remotes) = self.capture().await?;
            let captured = match namespace {
                RegistrationNamespace::Document => documents,
                RegistrationNamespace::Remote => remotes,
            };
            let registrations: Vec<SourceRegistration> =
                captured.registrations().values().cloned().collect();
            HostRegistrationSnapshot::from_complete_host_inventory(
                namespace,
                captured.deployment_revision(),
                registrations,
            )
        })
    }
}
