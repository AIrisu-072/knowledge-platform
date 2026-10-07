//! Principal/device-local Workspace broker for the bounded Runtime Contract.
//!
//! The desktop shell owns the window and the native picker; this crate owns
//! the protected local registry, managed roots, explicit folder bindings and
//! every filesystem access below them. Callers only ever see logical
//! Workspace IDs/names, opaque binding/selection/handle IDs and relative
//! locators. There is no general path, shell, process or executable API.

pub mod base64;
mod error;
pub mod locator;
mod registry;
pub mod wire;

#[cfg(unix)]
#[path = "platform_unix.rs"]
mod platform;
#[cfg(not(unix))]
#[path = "platform_unsupported.rs"]
mod platform;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use error::err;
pub use error::{RuntimeError, RuntimeErrorCode, RuntimeErrorReason, RuntimeResult};
use locator::{normalize_workspace_name, validate_locator, validate_name, validate_operation_id};
use platform::{Dir, Kind, Stat};
use registry::{
    BindingRecord, BindingSource, OperationKind, OperationRecord, OperationState, RegistryFile,
    WorkspaceRecord,
};

/// Contract ceilings from the frozen Runtime Contract (resource safety, not SLOs).
pub const MAX_ENTRY_PAGE: usize = 100;
pub const MAX_READ_RANGE: u64 = 1024 * 1024;
pub const MAX_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_CREATE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_READ_HANDLES: usize = 4;
pub const MAX_RETAINED_SNAPSHOT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SCANNED_ENTRIES: usize = 10_000;
const MAX_SELECTIONS: usize = 4;
const MAX_WORKSPACES: usize = 256;
const MAX_EXPLICIT_BINDINGS: usize = 32;
const MAX_LABEL_CHARS: usize = 80;
const MANAGED_DIR: &str = "managed";
const LOCK_FILE: &str = ".lock";
const SCOPE: &str = "principal_device_local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub local_resources: Availability,
    pub native_directory_picker: Availability,
    pub managed_workspace: Availability,
    pub multi_window: bool,
    pub sidecar: bool,
}

impl Capabilities {
    /// Capabilities when no runtime could be opened (browser parity).
    pub const fn unavailable() -> Self {
        Self {
            local_resources: Availability::Unavailable,
            native_directory_picker: Availability::Unavailable,
            managed_workspace: Availability::Unavailable,
            multi_window: false,
            sidecar: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextRef {
    pub effective_context_revision: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalRef {
    pub binding_id: String,
    pub locator: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingView {
    pub binding_id: String,
    /// `managed` or `explicit`; policy-derived bindings are server-owned.
    pub source: String,
    /// Presentation label: the leaf folder name the user chose, or the
    /// managed-root label. Never a path or locator.
    pub label: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub workspace_id: String,
    pub name: String,
    pub revision: String,
    pub effective_context_revision: String,
    pub scope: String,
    pub managed_binding_id: String,
    pub bindings: Vec<BindingView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeWorkspaceReceipt {
    pub operation_id: String,
    pub workspace_id: String,
    pub managed_binding_id: String,
    pub runtime_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreated {
    pub receipt: RuntimeWorkspaceReceipt,
    pub workspace: WorkspaceView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RuntimeWorkspaceOutcome {
    Ready { receipt: RuntimeWorkspaceReceipt },
    Pending,
    NotFound,
    Unavailable,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySelection {
    pub selection_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingReceipt {
    pub operation_id: String,
    pub binding_id: String,
    pub runtime_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingAttached {
    pub receipt: BindingReceipt,
    pub workspace: WorkspaceView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub locator: Vec<String>,
    pub name: String,
    pub kind: EntryKind,
    pub file_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPage {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<String>,
    /// Entries skipped because they are not addressable (symlinks, special
    /// files, non-UTF-8 or Windows-invalid names). Never silently "complete".
    pub omitted_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadHandle {
    pub read_handle_id: String,
    pub content_generation: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BytePage {
    #[serde(rename = "bytesBase64", serialize_with = "serialize_base64")]
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub content_generation: String,
    pub eof: bool,
}

fn serialize_base64<S: serde::Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&base64::encode(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReceipt {
    pub operation_id: String,
    pub r#ref: LocalRef,
    pub file_identity: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub handle_lease: Duration,
    pub selection_ttl: Duration,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            handle_lease: Duration::from_secs(300),
            selection_ttl: Duration::from_secs(300),
        }
    }
}

/// Exclusive native-picker reservation. Exactly one picker may be open; the
/// shell must finish it with the chosen folder or `None` (cancel).
#[derive(Debug)]
pub struct PickerTicket {
    id: u64,
    workspace_id: String,
}

#[derive(Debug)]
struct Selection {
    workspace_id: String,
    path: PathBuf,
    identity: (u64, u64),
    label: String,
    created: Instant,
}

#[derive(Debug)]
struct HandleEntry {
    workspace_id: String,
    context_revision: String,
    binding_id: String,
    bytes: Arc<[u8]>,
    generation: String,
    expires: Instant,
}

#[derive(Debug)]
struct State {
    registry: RegistryFile,
    key: [u8; 32],
    picker: Option<u64>,
    picker_sequence: u64,
    selections: HashMap<String, Selection>,
    handles: HashMap<String, HandleEntry>,
}

#[derive(Debug)]
pub struct LocalWorkspaceRuntime {
    state_root: PathBuf,
    state_identity: (u64, u64),
    options: RuntimeOptions,
    state: Mutex<State>,
    _lock: platform::InstanceLock,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn random_id(prefix: &str) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}

fn keyed(key: &[u8; 32], label: &str, parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(label.as_bytes());
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hex(&hasher.finalize()[..16])
}

fn file_identity(key: &[u8; 32], stat: &Stat) -> String {
    let (birth_s, birth_ns) = stat.birth.unwrap_or((0, -1));
    keyed(
        key,
        "file",
        &[
            &stat.dev.to_le_bytes(),
            &stat.ino.to_le_bytes(),
            &birth_s.to_le_bytes(),
            &birth_ns.to_le_bytes(),
        ],
    )
}

fn operation_digest(kind: OperationKind, value: serde_json::Value) -> String {
    sha256_hex(serde_json::json!([kind, value]).to_string().as_bytes())
}

/// The effective context changes only when the bound resource set changes;
/// a logical rename keeps in-flight resource contexts valid.
fn context_revision(record: &WorkspaceRecord) -> String {
    format!("c{}", record.revision_binding)
}

fn label_of(path: &Path) -> String {
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".to_owned());
    label.chars().take(MAX_LABEL_CHARS).collect()
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A panic while holding the lock leaves only in-memory caches; the
    // registry on disk is the authority, so continuing is safe.
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn not_found<T>() -> RuntimeResult<T> {
    Err(RuntimeError::new(RuntimeErrorCode::NotFound))
}

fn mismatch<T>() -> RuntimeResult<T> {
    err(
        RuntimeErrorCode::Conflict,
        RuntimeErrorReason::OperationMismatch,
    )
}

impl LocalWorkspaceRuntime {
    pub fn open(state_root: impl AsRef<Path>) -> RuntimeResult<Self> {
        Self::open_with_options(state_root, RuntimeOptions::default())
    }

    pub fn open_with_options(
        state_root: impl AsRef<Path>,
        options: RuntimeOptions,
    ) -> RuntimeResult<Self> {
        let requested = state_root.as_ref();
        platform::create_private_dir_all(requested)?;
        let state_root = std::fs::canonicalize(requested).map_err(|_| {
            RuntimeError::with(RuntimeErrorCode::Unavailable, RuntimeErrorReason::Io)
        })?;
        let instance_lock = platform::InstanceLock::acquire(&state_root.join(LOCK_FILE))?;
        platform::create_private_dir_all(&state_root.join(MANAGED_DIR))?;
        let state_dir = Dir::open_absolute(&state_root)?;
        let registry = match registry::load(&state_root)? {
            Some(registry) => registry,
            None => {
                let key = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                let registry = RegistryFile::new(key);
                registry::save(&state_root, &registry).map_err(|_| {
                    RuntimeError::with(
                        RuntimeErrorCode::Unavailable,
                        RuntimeErrorReason::RegistryWriteFailed,
                    )
                })?;
                registry
            }
        };
        let key: [u8; 32] = unhex(&registry.identity_key)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| {
                RuntimeError::with(
                    RuntimeErrorCode::Unavailable,
                    RuntimeErrorReason::RegistryUnreadable,
                )
            })?;
        Ok(Self {
            state_identity: state_dir.stat.identity(),
            state_root,
            options,
            state: Mutex::new(State {
                registry,
                key,
                picker: None,
                picker_sequence: 0,
                selections: HashMap::new(),
                handles: HashMap::new(),
            }),
            _lock: instance_lock,
        })
    }

    /// `picker_available` is reported by the shell that owns the native dialog.
    pub fn capabilities(&self, picker_available: bool) -> Capabilities {
        let available = |yes: bool| {
            if yes && platform::SUPPORTED {
                Availability::Available
            } else {
                Availability::Unavailable
            }
        };
        Capabilities {
            local_resources: available(true),
            native_directory_picker: available(picker_available),
            managed_workspace: available(true),
            multi_window: false,
            sidecar: false,
        }
    }

    // ----- views -------------------------------------------------------

    fn managed_path(&self, binding_id: &str) -> PathBuf {
        self.state_root.join(MANAGED_DIR).join(binding_id)
    }

    fn open_binding_root(&self, binding: &BindingRecord) -> RuntimeResult<Dir> {
        let path = match (&binding.source, &binding.path) {
            (BindingSource::Managed, _) => self.managed_path(&binding.id),
            (BindingSource::Explicit, Some(path)) => path.clone(),
            (BindingSource::Explicit, None) => {
                return err(
                    RuntimeErrorCode::Unavailable,
                    RuntimeErrorReason::FolderReplaced,
                );
            }
        };
        let replaced = || {
            RuntimeError::with(
                RuntimeErrorCode::Unavailable,
                RuntimeErrorReason::FolderReplaced,
            )
        };
        let dir = Dir::open_absolute(&path).map_err(|_| replaced())?;
        if dir.stat.identity() != (binding.dev, binding.ino) {
            return Err(replaced());
        }
        Ok(dir)
    }

    fn view(&self, record: &WorkspaceRecord) -> WorkspaceView {
        WorkspaceView {
            workspace_id: record.id.clone(),
            name: record.name.clone(),
            revision: format!("r{}", record.revision),
            effective_context_revision: context_revision(record),
            scope: SCOPE.to_owned(),
            managed_binding_id: record.managed_binding_id.clone(),
            bindings: record
                .bindings
                .iter()
                .map(|binding| BindingView {
                    binding_id: binding.id.clone(),
                    source: match binding.source {
                        BindingSource::Managed => "managed",
                        BindingSource::Explicit => "explicit",
                    }
                    .to_owned(),
                    label: binding.label.clone(),
                    available: self.open_binding_root(binding).is_ok(),
                })
                .collect(),
        }
    }

    fn persist(&self, state: &State) -> RuntimeResult<()> {
        registry::save(&self.state_root, &state.registry)
    }

    fn check_context<'a>(
        state: &'a State,
        context: &ContextRef,
    ) -> RuntimeResult<&'a WorkspaceRecord> {
        let Some(record) = state.registry.workspace(&context.workspace_id) else {
            return not_found();
        };
        if context_revision(record) != context.effective_context_revision {
            return Err(RuntimeError::new(RuntimeErrorCode::StaleContext));
        }
        Ok(record)
    }

    pub fn list_workspaces(&self) -> RuntimeResult<Vec<WorkspaceView>> {
        let state = lock(&self.state);
        Ok(state
            .registry
            .workspaces
            .iter()
            .map(|record| self.view(record))
            .collect())
    }

    // ----- logical Workspace -------------------------------------------

    pub fn create_workspace(
        &self,
        name: &str,
        operation_id: &str,
    ) -> RuntimeResult<WorkspaceCreated> {
        validate_operation_id(operation_id)?;
        let name = normalize_workspace_name(name)?;
        let digest = operation_digest(OperationKind::CreateWorkspace, serde_json::json!(name));
        let mut state = lock(&self.state);
        let (workspace_id, managed_binding_id) = match state.registry.operation(operation_id) {
            Some(op) if op.kind != OperationKind::CreateWorkspace || op.digest != digest => {
                return mismatch();
            }
            Some(op) => stored_workspace_ids(op)?,
            None => {
                if state.registry.workspaces.len() >= MAX_WORKSPACES {
                    return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooMany);
                }
                // Reserve the identities before touching the filesystem so a
                // lost response or crash recovers the same root, never another.
                let ids = (random_id("w_"), random_id("b_"));
                state.registry.upsert_operation(OperationRecord {
                    id: operation_id.to_owned(),
                    kind: OperationKind::CreateWorkspace,
                    digest,
                    state: OperationState::Pending,
                    result: serde_json::json!({"workspaceId": ids.0, "managedBindingId": ids.1, "name": name}),
                });
                self.persist(&state).map_err(|_| {
                    RuntimeError::with(
                        RuntimeErrorCode::Unavailable,
                        RuntimeErrorReason::RegistryWriteFailed,
                    )
                })?;
                ids
            }
        };
        self.complete_workspace_creation(
            &mut state,
            operation_id,
            &workspace_id,
            &managed_binding_id,
        )
    }

    fn complete_workspace_creation(
        &self,
        state: &mut State,
        operation_id: &str,
        workspace_id: &str,
        managed_binding_id: &str,
    ) -> RuntimeResult<WorkspaceCreated> {
        let receipt = RuntimeWorkspaceReceipt {
            operation_id: operation_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            managed_binding_id: managed_binding_id.to_owned(),
            runtime_revision: "1".to_owned(),
        };
        if let Some(record) = state.registry.workspace(workspace_id)
            && state
                .registry
                .operation(operation_id)
                .is_some_and(|op| op.state == OperationState::Completed)
        {
            return Ok(WorkspaceCreated {
                receipt,
                workspace: self.view(record),
            });
        }
        let name = state
            .registry
            .operation(operation_id)
            .and_then(|op| op.result.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let managed_parent = Dir::open_absolute(&self.state_root.join(MANAGED_DIR))?;
        match managed_parent.make_private_child_dir(managed_binding_id) {
            Ok(()) => {}
            Err(error) if error.reason == Some(RuntimeErrorReason::AlreadyExists) => {}
            Err(error) => return Err(error),
        }
        // Opened no-follow: a planted symlink in place of the root is refused.
        let root = managed_parent.open_child_dir(managed_binding_id)?;
        if state.registry.workspace(workspace_id).is_none() {
            state.registry.workspaces.push(WorkspaceRecord {
                id: workspace_id.to_owned(),
                name,
                revision: 1,
                revision_binding: 0,
                managed_binding_id: managed_binding_id.to_owned(),
                bindings: vec![BindingRecord {
                    id: managed_binding_id.to_owned(),
                    source: BindingSource::Managed,
                    label: "管理フォルダー".to_owned(),
                    path: None,
                    dev: root.stat.dev,
                    ino: root.stat.ino,
                }],
            });
        }
        if let Some(op) = state
            .registry
            .operations
            .iter_mut()
            .find(|op| op.id == operation_id)
        {
            op.state = OperationState::Completed;
        }
        self.persist(state)?;
        let record = state
            .registry
            .workspace(workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
        Ok(WorkspaceCreated {
            receipt,
            workspace: self.view(record),
        })
    }

    pub fn recover_workspace(&self, operation_id: &str) -> RuntimeWorkspaceOutcome {
        if validate_operation_id(operation_id).is_err() {
            return RuntimeWorkspaceOutcome::NotFound;
        }
        let mut state = lock(&self.state);
        let ids = match state.registry.operation(operation_id) {
            Some(op) if op.kind == OperationKind::CreateWorkspace => stored_workspace_ids(op),
            _ => return RuntimeWorkspaceOutcome::NotFound,
        };
        let Ok((workspace_id, managed_binding_id)) = ids else {
            return RuntimeWorkspaceOutcome::OutcomeUnknown;
        };
        match self.complete_workspace_creation(
            &mut state,
            operation_id,
            &workspace_id,
            &managed_binding_id,
        ) {
            Ok(created) => RuntimeWorkspaceOutcome::Ready {
                receipt: created.receipt,
            },
            Err(error) if error.code == RuntimeErrorCode::OutcomeUnknown => {
                RuntimeWorkspaceOutcome::OutcomeUnknown
            }
            Err(_) => RuntimeWorkspaceOutcome::Unavailable,
        }
    }

    pub fn rename_workspace(
        &self,
        context: &ContextRef,
        name: &str,
        operation_id: &str,
    ) -> RuntimeResult<WorkspaceView> {
        validate_operation_id(operation_id)?;
        let name = normalize_workspace_name(name)?;
        let digest = operation_digest(
            OperationKind::RenameWorkspace,
            serde_json::json!([context.workspace_id, name]),
        );
        let mut state = lock(&self.state);
        if let Some(op) = state.registry.operation(operation_id) {
            if op.kind != OperationKind::RenameWorkspace || op.digest != digest {
                return mismatch();
            }
            let Some(record) = state.registry.workspace(&context.workspace_id) else {
                return not_found();
            };
            return Ok(self.view(record));
        }
        Self::check_context(&state, context)?;
        let record = state
            .registry
            .workspace_mut(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        record.name = name;
        record.revision += 1;
        state.registry.upsert_operation(OperationRecord {
            id: operation_id.to_owned(),
            kind: OperationKind::RenameWorkspace,
            digest,
            state: OperationState::Completed,
            result: serde_json::Value::Null,
        });
        self.persist(&state)?;
        let record = state
            .registry
            .workspace(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
        Ok(self.view(record))
    }

    // ----- native picker selection and explicit bindings ---------------

    pub fn begin_directory_selection(&self, context: &ContextRef) -> RuntimeResult<PickerTicket> {
        let mut state = lock(&self.state);
        Self::check_context(&state, context)?;
        if state.picker.is_some() {
            return err(RuntimeErrorCode::Conflict, RuntimeErrorReason::PickerBusy);
        }
        state.picker_sequence += 1;
        let id = state.picker_sequence;
        state.picker = Some(id);
        Ok(PickerTicket {
            id,
            workspace_id: context.workspace_id.clone(),
        })
    }

    /// Complete the picker. `None` is a user cancellation: nothing changes.
    /// The chosen path stays inside the broker; callers get a single-use,
    /// expiring selection ID bound to the Workspace.
    pub fn finish_directory_selection(
        &self,
        ticket: PickerTicket,
        chosen: Option<PathBuf>,
    ) -> RuntimeResult<Option<DirectorySelection>> {
        let mut state = lock(&self.state);
        if state.picker != Some(ticket.id) {
            return err(RuntimeErrorCode::Conflict, RuntimeErrorReason::PickerBusy);
        }
        state.picker = None;
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        if state.registry.workspace(&ticket.workspace_id).is_none() {
            return not_found();
        }
        let canonical = std::fs::canonicalize(&chosen)
            .map_err(|_| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        if canonical.starts_with(&self.state_root) {
            return err(
                RuntimeErrorCode::Denied,
                RuntimeErrorReason::ProtectedLocation,
            );
        }
        let dir = Dir::open_absolute(&canonical)?;
        let ttl = self.options.selection_ttl;
        state.selections.retain(|_, s| s.created.elapsed() < ttl);
        while state.selections.len() >= MAX_SELECTIONS {
            let oldest = state
                .selections
                .iter()
                .min_by_key(|(_, s)| s.created)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => state.selections.remove(&id),
                None => break,
            };
        }
        let selection_id = random_id("s_");
        state.selections.insert(
            selection_id.clone(),
            Selection {
                workspace_id: ticket.workspace_id,
                label: label_of(&canonical),
                path: canonical,
                identity: dir.stat.identity(),
                created: Instant::now(),
            },
        );
        Ok(Some(DirectorySelection { selection_id }))
    }

    pub fn attach_directory(
        &self,
        context: &ContextRef,
        selection: &DirectorySelection,
        operation_id: &str,
    ) -> RuntimeResult<BindingAttached> {
        validate_operation_id(operation_id)?;
        let digest = operation_digest(
            OperationKind::AttachDirectory,
            serde_json::json!([context.workspace_id, selection.selection_id]),
        );
        let mut state = lock(&self.state);
        if let Some(op) = state.registry.operation(operation_id) {
            if op.kind != OperationKind::AttachDirectory || op.digest != digest {
                return mismatch();
            }
            let receipt: BindingReceipt = serde_json::from_value(op.result.clone())
                .map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
            let Some(record) = state.registry.workspace(&context.workspace_id) else {
                return not_found();
            };
            return Ok(BindingAttached {
                receipt,
                workspace: self.view(record),
            });
        }
        Self::check_context(&state, context)?;
        let ttl = self.options.selection_ttl;
        let valid = state
            .selections
            .get(&selection.selection_id)
            .is_some_and(|s| s.workspace_id == context.workspace_id && s.created.elapsed() < ttl);
        if !valid {
            return err(
                RuntimeErrorCode::NotFound,
                RuntimeErrorReason::SelectionExpired,
            );
        }
        let Some(chosen) = state.selections.remove(&selection.selection_id) else {
            return not_found();
        };
        // The folder must still be the very directory that was picked.
        let dir = Dir::open_absolute(&chosen.path)?;
        if dir.stat.identity() != chosen.identity || chosen.identity == self.state_identity {
            return err(
                RuntimeErrorCode::Conflict,
                RuntimeErrorReason::FolderReplaced,
            );
        }
        let record = state
            .registry
            .workspace(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        if record
            .bindings
            .iter()
            .any(|b| (b.dev, b.ino) == chosen.identity)
        {
            return err(RuntimeErrorCode::Conflict, RuntimeErrorReason::AlreadyBound);
        }
        if record
            .bindings
            .iter()
            .filter(|b| b.source == BindingSource::Explicit)
            .count()
            >= MAX_EXPLICIT_BINDINGS
        {
            return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooMany);
        }
        let binding_id = random_id("b_");
        let record = state
            .registry
            .workspace_mut(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        record.bindings.push(BindingRecord {
            id: binding_id.clone(),
            source: BindingSource::Explicit,
            label: chosen.label,
            path: Some(chosen.path),
            dev: chosen.identity.0,
            ino: chosen.identity.1,
        });
        record.revision += 1;
        record.revision_binding += 1;
        let receipt = BindingReceipt {
            operation_id: operation_id.to_owned(),
            binding_id,
            runtime_revision: format!("r{}", record.revision),
        };
        state.registry.upsert_operation(OperationRecord {
            id: operation_id.to_owned(),
            kind: OperationKind::AttachDirectory,
            digest,
            state: OperationState::Completed,
            result: serde_json::to_value(&receipt)
                .map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?,
        });
        self.persist(&state)?;
        let record = state
            .registry
            .workspace(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
        Ok(BindingAttached {
            receipt,
            workspace: self.view(record),
        })
    }

    /// Remove an explicit binding reference. Folder contents are untouched.
    pub fn detach_directory(
        &self,
        context: &ContextRef,
        binding_id: &str,
        operation_id: &str,
    ) -> RuntimeResult<WorkspaceView> {
        validate_operation_id(operation_id)?;
        let digest = operation_digest(
            OperationKind::DetachDirectory,
            serde_json::json!([context.workspace_id, binding_id]),
        );
        let mut state = lock(&self.state);
        if let Some(op) = state.registry.operation(operation_id) {
            if op.kind != OperationKind::DetachDirectory || op.digest != digest {
                return mismatch();
            }
            let Some(record) = state.registry.workspace(&context.workspace_id) else {
                return not_found();
            };
            return Ok(self.view(record));
        }
        let record = Self::check_context(&state, context)?;
        match record.bindings.iter().find(|b| b.id == binding_id) {
            None => return not_found(),
            Some(b) if b.source == BindingSource::Managed => {
                return err(RuntimeErrorCode::Denied, RuntimeErrorReason::ManagedBinding);
            }
            Some(_) => {}
        }
        let record = state
            .registry
            .workspace_mut(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        record.bindings.retain(|b| b.id != binding_id);
        record.revision += 1;
        record.revision_binding += 1;
        state.handles.retain(|_, h| h.binding_id != binding_id);
        state.registry.upsert_operation(OperationRecord {
            id: operation_id.to_owned(),
            kind: OperationKind::DetachDirectory,
            digest,
            state: OperationState::Completed,
            result: serde_json::Value::Null,
        });
        self.persist(&state)?;
        let record = state
            .registry
            .workspace(&context.workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
        Ok(self.view(record))
    }

    // ----- confined resource access ------------------------------------

    fn binding<'a>(
        record: &'a WorkspaceRecord,
        binding_id: &str,
    ) -> RuntimeResult<&'a BindingRecord> {
        record
            .bindings
            .iter()
            .find(|b| b.id == binding_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))
    }

    fn walk(&self, root: Dir, locator: &[String]) -> RuntimeResult<Dir> {
        let mut current = root;
        for name in locator {
            let next = current.open_child_dir(name)?;
            if next.stat.identity() == self.state_identity {
                return err(
                    RuntimeErrorCode::Denied,
                    RuntimeErrorReason::ProtectedLocation,
                );
            }
            current = next;
        }
        Ok(current)
    }

    pub fn list_entries(
        &self,
        context: &ContextRef,
        target: &LocalRef,
        cursor: Option<&str>,
    ) -> RuntimeResult<EntryPage> {
        validate_locator(&target.locator)?;
        let state = lock(&self.state);
        let record = Self::check_context(&state, context)?;
        let binding = Self::binding(record, &target.binding_id)?;
        let dir = self.walk(self.open_binding_root(binding)?, &target.locator)?;
        let tag = keyed(
            &state.key,
            "cursor",
            &[&dir.stat.dev.to_le_bytes(), &dir.stat.ino.to_le_bytes()],
        );
        let after = match cursor {
            None => None,
            Some(cursor) => {
                let invalid = || {
                    RuntimeError::with(
                        RuntimeErrorCode::InvalidLocator,
                        RuntimeErrorReason::InvalidCursor,
                    )
                };
                let (cursor_tag, name) = cursor.split_once('.').ok_or_else(invalid)?;
                if cursor_tag != tag {
                    return Err(invalid());
                }
                Some(unhex(name).ok_or_else(invalid)?)
            }
        };
        let mut names = dir.entry_names(MAX_SCANNED_ENTRIES)?;
        names.sort();
        let start = after.map_or(0, |after| names.partition_point(|name| *name <= after));
        let mut entries = Vec::new();
        let mut omitted = 0u32;
        let mut next_cursor = None;
        for (index, raw) in names.iter().enumerate().skip(start) {
            if entries.len() == MAX_ENTRY_PAGE {
                next_cursor = Some(format!("{tag}.{}", hex(&names[index - 1])));
                break;
            }
            let Some(name) = std::str::from_utf8(raw)
                .ok()
                .filter(|n| validate_name(n).is_ok())
            else {
                omitted += 1;
                continue;
            };
            let Ok(stat) = dir.stat_child(name) else {
                omitted += 1;
                continue;
            };
            let kind = match stat.kind {
                Kind::Directory if stat.identity() == self.state_identity => continue,
                Kind::Directory => EntryKind::Directory,
                Kind::Regular => EntryKind::File,
                Kind::Symlink | Kind::Other => {
                    omitted += 1;
                    continue;
                }
            };
            let mut locator = target.locator.clone();
            locator.push(name.to_owned());
            entries.push(Entry {
                locator,
                name: name.to_owned(),
                kind,
                file_identity: file_identity(&state.key, &stat),
            });
        }
        Ok(EntryPage {
            entries,
            next_cursor,
            omitted_count: omitted,
        })
    }

    fn open_parent(
        &self,
        record: &WorkspaceRecord,
        target: &LocalRef,
    ) -> RuntimeResult<(Dir, String)> {
        validate_locator(&target.locator)?;
        let Some((name, parents)) = target.locator.split_last() else {
            return err(
                RuntimeErrorCode::InvalidLocator,
                RuntimeErrorReason::InvalidName,
            );
        };
        let binding = Self::binding(record, &target.binding_id)?;
        let parent = self.walk(self.open_binding_root(binding)?, parents)?;
        Ok((parent, name.clone()))
    }

    pub fn open_read(
        &self,
        context: &ContextRef,
        target: &LocalRef,
        expected_file_identity: &str,
    ) -> RuntimeResult<ReadHandle> {
        let mut state = lock(&self.state);
        let record = Self::check_context(&state, context)?;
        let (parent, name) = self.open_parent(record, target)?;
        let (file, stat) = parent.open_child_file(&name)?;
        if stat.nlink != 1 {
            return err(RuntimeErrorCode::Denied, RuntimeErrorReason::LinkedFile);
        }
        if stat.size > MAX_SNAPSHOT_BYTES {
            return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooLarge);
        }
        if file_identity(&state.key, &stat) != expected_file_identity {
            return err(
                RuntimeErrorCode::Conflict,
                RuntimeErrorReason::ConcurrentChange,
            );
        }
        let now = Instant::now();
        state.handles.retain(|_, h| h.expires > now);
        let retained: u64 = state.handles.values().map(|h| h.bytes.len() as u64).sum();
        if state.handles.len() >= MAX_READ_HANDLES
            || retained + stat.size > MAX_RETAINED_SNAPSHOT_BYTES
        {
            return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooMany);
        }
        let bytes = platform::read_stable(&file, &stat)?;
        let generation = keyed(
            &state.key,
            "generation",
            &[
                file_identity(&state.key, &stat).as_bytes(),
                &stat.size.to_le_bytes(),
                &stat.mtime.0.to_le_bytes(),
                &stat.mtime.1.to_le_bytes(),
                &stat.ctime.0.to_le_bytes(),
                &stat.ctime.1.to_le_bytes(),
                Sha256::digest(&bytes).as_slice(),
            ],
        );
        let read_handle_id = random_id("h_");
        let size_bytes = bytes.len() as u64;
        state.handles.insert(
            read_handle_id.clone(),
            HandleEntry {
                workspace_id: context.workspace_id.clone(),
                context_revision: context.effective_context_revision.clone(),
                binding_id: target.binding_id.clone(),
                bytes: bytes.into(),
                generation: generation.clone(),
                expires: now + self.options.handle_lease,
            },
        );
        Ok(ReadHandle {
            read_handle_id,
            content_generation: generation,
            size_bytes,
        })
    }

    pub fn read_file(
        &self,
        context: &ContextRef,
        read_handle_id: &str,
        offset: u64,
        length: u64,
    ) -> RuntimeResult<BytePage> {
        let mut state = lock(&self.state);
        Self::check_context(&state, context)?;
        if length == 0 || length > MAX_READ_RANGE {
            return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooLarge);
        }
        let now = Instant::now();
        state.handles.retain(|_, h| h.expires > now);
        let Some(handle) = state.handles.get(read_handle_id) else {
            return err(
                RuntimeErrorCode::NotFound,
                RuntimeErrorReason::HandleExpired,
            );
        };
        if handle.workspace_id != context.workspace_id
            || handle.context_revision != context.effective_context_revision
        {
            return not_found();
        }
        let size = handle.bytes.len() as u64;
        if offset > size {
            return err(
                RuntimeErrorCode::InvalidLocator,
                RuntimeErrorReason::TooLarge,
            );
        }
        let end = offset.saturating_add(length).min(size);
        Ok(BytePage {
            bytes: handle.bytes[offset as usize..end as usize].to_vec(),
            offset,
            content_generation: handle.generation.clone(),
            eof: end == size,
        })
    }

    pub fn close_read(&self, context: &ContextRef, read_handle_id: &str) -> RuntimeResult<()> {
        let mut state = lock(&self.state);
        if state
            .handles
            .get(read_handle_id)
            .is_some_and(|h| h.workspace_id == context.workspace_id)
        {
            state.handles.remove(read_handle_id);
        }
        Ok(())
    }

    /// Exclusive, no-overwrite creation below a binding. An exact retry of the
    /// same operation returns its stored receipt only while the created file
    /// still has the recorded identity; uncertain completion is never retried
    /// under a new name.
    pub fn create_file(
        &self,
        context: &ContextRef,
        parent: &LocalRef,
        name: &str,
        bytes: &[u8],
        operation_id: &str,
    ) -> RuntimeResult<FileReceipt> {
        validate_operation_id(operation_id)?;
        validate_locator(&parent.locator)?;
        validate_name(name)?;
        if bytes.len() > MAX_CREATE_BYTES {
            return err(RuntimeErrorCode::Limit, RuntimeErrorReason::TooLarge);
        }
        let mut target = parent.clone();
        target.locator.push(name.to_owned());
        validate_locator(&target.locator)?;
        let content_sha256 = sha256_hex(bytes);
        let digest = operation_digest(
            OperationKind::CreateFile,
            serde_json::json!([context.workspace_id, target, content_sha256, bytes.len()]),
        );
        let mut state = lock(&self.state);
        if let Some(op) = state.registry.operation(operation_id).cloned() {
            if op.kind != OperationKind::CreateFile || op.digest != digest {
                return mismatch();
            }
            let record = state
                .registry
                .workspace(&context.workspace_id)
                .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
            let (dir, leaf) = self.open_parent(record, &target)?;
            if op.state == OperationState::Completed {
                let receipt: FileReceipt = serde_json::from_value(op.result)
                    .map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
                let current = dir.stat_child(&leaf)?;
                if file_identity(&state.key, &current) != receipt.file_identity {
                    return err(
                        RuntimeErrorCode::Conflict,
                        RuntimeErrorReason::ConcurrentChange,
                    );
                }
                return Ok(receipt);
            }
            // Pending after a crash or failed registry write: adopt only the
            // exact recorded content, otherwise the outcome stays uncertain.
            if let Ok((file, stat)) = dir.open_child_file(&leaf) {
                let existing = platform::read_stable(&file, &stat)?;
                if stat.nlink != 1 || sha256_hex(&existing) != content_sha256 {
                    return err(
                        RuntimeErrorCode::Conflict,
                        RuntimeErrorReason::AlreadyExists,
                    );
                }
                let receipt =
                    self.file_receipt(&state, operation_id, &target, &stat, &content_sha256);
                return self.complete_create(&mut state, operation_id, digest, receipt);
            }
        } else {
            Self::check_context(&state, context)?;
            state.registry.upsert_operation(OperationRecord {
                id: operation_id.to_owned(),
                kind: OperationKind::CreateFile,
                digest: digest.clone(),
                state: OperationState::Pending,
                result: serde_json::Value::Null,
            });
            self.persist(&state).map_err(|_| {
                RuntimeError::with(
                    RuntimeErrorCode::Unavailable,
                    RuntimeErrorReason::RegistryWriteFailed,
                )
            })?;
        }
        let result = self.create_new_file(
            &state,
            &context.workspace_id,
            &target,
            bytes,
            &content_sha256,
            operation_id,
        );
        match result {
            Ok(receipt) => self.complete_create(&mut state, operation_id, digest, receipt),
            Err(error) => {
                if error.code != RuntimeErrorCode::OutcomeUnknown {
                    // Definite failure: nothing was created, forget the attempt.
                    state.registry.operations.retain(|op| op.id != operation_id);
                    let _ = self.persist(&state);
                }
                Err(error)
            }
        }
    }

    fn file_receipt(
        &self,
        state: &State,
        operation_id: &str,
        target: &LocalRef,
        stat: &Stat,
        content_sha256: &str,
    ) -> FileReceipt {
        FileReceipt {
            operation_id: operation_id.to_owned(),
            r#ref: target.clone(),
            file_identity: file_identity(&state.key, stat),
            size_bytes: stat.size,
            sha256: content_sha256.to_owned(),
        }
    }

    fn create_new_file(
        &self,
        state: &State,
        workspace_id: &str,
        target: &LocalRef,
        bytes: &[u8],
        content_sha256: &str,
        operation_id: &str,
    ) -> RuntimeResult<FileReceipt> {
        let record = state
            .registry
            .workspace(workspace_id)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorCode::NotFound))?;
        let (dir, leaf) = self.open_parent(record, target)?;
        let mut file = dir.create_child_file(&leaf)?;
        let created = platform::fstat_file(&file)?;
        let written = platform::write_all_durable(&mut file, bytes);
        // Post-verify confinement: the parent reached again from the binding
        // root must be the same directory the file was created in, and the
        // name must still denote the new inode. Otherwise remove our own file.
        let (again, _) = self.open_parent(record, target).map_err(|_| {
            RuntimeError::with(
                RuntimeErrorCode::Conflict,
                RuntimeErrorReason::ConcurrentChange,
            )
        })?;
        let still_ours = dir
            .stat_child(&leaf)
            .is_ok_and(|s| s.identity() == created.identity());
        if again.stat.identity() != dir.stat.identity() || !still_ours {
            if still_ours {
                let _ = dir.unlink_child_file(&leaf);
            }
            return err(
                RuntimeErrorCode::Conflict,
                RuntimeErrorReason::ConcurrentChange,
            );
        }
        if written.is_err() {
            return Err(RuntimeError::new(RuntimeErrorCode::OutcomeUnknown));
        }
        let stat = platform::fstat_file(&file)
            .map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?;
        Ok(self.file_receipt(state, operation_id, target, &stat, content_sha256))
    }

    fn complete_create(
        &self,
        state: &mut State,
        operation_id: &str,
        digest: String,
        receipt: FileReceipt,
    ) -> RuntimeResult<FileReceipt> {
        state.registry.upsert_operation(OperationRecord {
            id: operation_id.to_owned(),
            kind: OperationKind::CreateFile,
            digest,
            state: OperationState::Completed,
            result: serde_json::to_value(&receipt)
                .map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))?,
        });
        self.persist(state)?;
        Ok(receipt)
    }
}

fn stored_workspace_ids(op: &OperationRecord) -> RuntimeResult<(String, String)> {
    let get = |key: &str| {
        op.result
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    match (get("workspaceId"), get("managedBindingId")) {
        (Some(w), Some(b)) => Ok((w, b)),
        _ => Err(RuntimeError::new(RuntimeErrorCode::OutcomeUnknown)),
    }
}

#[cfg(all(test, unix))]
mod crash_recovery_tests {
    use super::*;

    fn pending(rt: &LocalWorkspaceRuntime, record: OperationRecord) {
        let mut state = lock(&rt.state);
        state.registry.upsert_operation(record);
        rt.persist(&state).unwrap();
    }

    #[test]
    fn pending_workspace_creation_recovers_the_reserved_root_once_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        {
            // Crash after reserving identities, before the managed root exists.
            let rt = LocalWorkspaceRuntime::open(&state).unwrap();
            pending(
                &rt,
                OperationRecord {
                    id: "op-crash".into(),
                    kind: OperationKind::CreateWorkspace,
                    digest: operation_digest(
                        OperationKind::CreateWorkspace,
                        serde_json::json!("名前"),
                    ),
                    state: OperationState::Pending,
                    result: serde_json::json!({"workspaceId": "w_reserved", "managedBindingId": "b_reserved", "name": "名前"}),
                },
            );
        }
        let rt = LocalWorkspaceRuntime::open(&state).unwrap();
        assert!(rt.list_workspaces().unwrap().is_empty());
        match rt.recover_workspace("op-crash") {
            RuntimeWorkspaceOutcome::Ready { receipt } => {
                assert_eq!(receipt.workspace_id, "w_reserved");
                assert_eq!(receipt.managed_binding_id, "b_reserved");
            }
            other => panic!("unexpected {other:?}"),
        }
        let again = rt.create_workspace("名前", "op-crash").unwrap();
        assert_eq!(again.receipt.managed_binding_id, "b_reserved");
        assert_eq!(rt.list_workspaces().unwrap().len(), 1);
        assert_eq!(
            std::fs::read_dir(state.join(MANAGED_DIR)).unwrap().count(),
            1
        );
    }

    #[test]
    fn pending_file_creation_adopts_only_the_exact_recorded_content() {
        let temp = tempfile::tempdir().unwrap();
        let rt = LocalWorkspaceRuntime::open(temp.path().join("state")).unwrap();
        let created = rt.create_workspace("w", "op-w").unwrap().workspace;
        let context = ContextRef {
            workspace_id: created.workspace_id.clone(),
            effective_context_revision: created.effective_context_revision.clone(),
        };
        let parent = LocalRef {
            binding_id: created.managed_binding_id.clone(),
            locator: vec![],
        };
        let root = rt.managed_path(&created.managed_binding_id);
        let record = |op: &str, name: &str, bytes: &[u8]| {
            let mut target = parent.clone();
            target.locator.push(name.to_owned());
            OperationRecord {
                id: op.into(),
                kind: OperationKind::CreateFile,
                digest: operation_digest(
                    OperationKind::CreateFile,
                    serde_json::json!([
                        created.workspace_id,
                        target,
                        sha256_hex(bytes),
                        bytes.len()
                    ]),
                ),
                state: OperationState::Pending,
                result: serde_json::Value::Null,
            }
        };
        // Written before the crash with exactly the requested bytes: adopted.
        pending(&rt, record("op-same", "same.txt", b"abc"));
        std::fs::write(root.join("same.txt"), b"abc").unwrap();
        let receipt = rt
            .create_file(&context, &parent, "same.txt", b"abc", "op-same")
            .unwrap();
        assert_eq!(receipt.size_bytes, 3);
        // A different file under that name is never claimed as ours.
        pending(&rt, record("op-other", "other.txt", b"abc"));
        std::fs::write(root.join("other.txt"), b"partial").unwrap();
        let error = rt
            .create_file(&context, &parent, "other.txt", b"abc", "op-other")
            .unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::Conflict);
        assert_eq!(std::fs::read(root.join("other.txt")).unwrap(), b"partial");
        // Nothing was written before the crash: the create proceeds once.
        pending(&rt, record("op-none", "none.txt", b"xyz"));
        rt.create_file(&context, &parent, "none.txt", b"xyz", "op-none")
            .unwrap();
        assert_eq!(std::fs::read(root.join("none.txt")).unwrap(), b"xyz");
    }
}
