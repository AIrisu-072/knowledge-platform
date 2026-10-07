//! Protected local registry persisted under the runtime state root.
//!
//! Physical paths of explicit bindings live only here, never in a value
//! returned to the frontend. Writes are atomic (temporary file, fsync,
//! rename, directory fsync). An unreadable registry fails closed instead of
//! being reset, so bindings are never silently lost.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{RuntimeError, RuntimeErrorCode, RuntimeErrorReason, RuntimeResult};

pub const SCHEMA: u32 = 1;
pub const REGISTRY_FILE: &str = "registry.json";
pub const MAX_OPERATIONS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingSource {
    Managed,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingRecord {
    pub id: String,
    pub source: BindingSource,
    pub label: String,
    /// Absolute path for explicit bindings; managed roots are derived from the
    /// state root and the binding ID instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub dev: u64,
    pub ino: u64,
    /// Creation time when the filesystem reports it (absent in old records).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub revision: u64,
    /// Incremented whenever the bound resource set changes.
    #[serde(default)]
    pub revision_binding: u64,
    pub managed_binding_id: String,
    pub bindings: Vec<BindingRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    CreateWorkspace,
    RenameWorkspace,
    AttachDirectory,
    DetachDirectory,
    CreateFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: String,
    pub kind: OperationKind,
    pub digest: String,
    pub state: OperationState,
    /// Kind-specific stored result (IDs/receipt), never a physical path.
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryFile {
    pub schema: u32,
    pub identity_key: String,
    pub workspaces: Vec<WorkspaceRecord>,
    pub operations: Vec<OperationRecord>,
}

impl RegistryFile {
    pub fn new(identity_key: String) -> Self {
        Self {
            schema: SCHEMA,
            identity_key,
            workspaces: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub fn operation(&self, id: &str) -> Option<&OperationRecord> {
        self.operations.iter().find(|op| op.id == id)
    }

    pub fn upsert_operation(&mut self, record: OperationRecord) {
        if let Some(existing) = self.operations.iter_mut().find(|op| op.id == record.id) {
            *existing = record;
            return;
        }
        self.operations.push(record);
        // Bounded retry memory. Workspace creations are never forgotten (they
        // are bounded by the Workspace limit), so the same operation ID can
        // never create a second Workspace or managed root. Among the rest the
        // oldest completed record goes first, then the oldest pending one.
        while self.operations.len() > MAX_OPERATIONS {
            let evictable = |op: &&OperationRecord| op.kind != OperationKind::CreateWorkspace;
            let index = self
                .operations
                .iter()
                .position(|op| evictable(&op) && op.state == OperationState::Completed)
                .or_else(|| self.operations.iter().position(|op| evictable(&op)));
            match index {
                Some(index) => {
                    self.operations.remove(index);
                }
                None => break,
            }
        }
    }

    pub fn workspace(&self, id: &str) -> Option<&WorkspaceRecord> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn workspace_mut(&mut self, id: &str) -> Option<&mut WorkspaceRecord> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }
}

fn unreadable() -> RuntimeError {
    RuntimeError::with(
        RuntimeErrorCode::Unavailable,
        RuntimeErrorReason::RegistryUnreadable,
    )
}

fn write_failed() -> RuntimeError {
    RuntimeError::with(
        RuntimeErrorCode::OutcomeUnknown,
        RuntimeErrorReason::RegistryWriteFailed,
    )
}

pub fn load(state_root: &Path) -> RuntimeResult<Option<RegistryFile>> {
    let path = state_root.join(REGISTRY_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(unreadable()),
    };
    let registry: RegistryFile = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
    if registry.schema != SCHEMA || registry.identity_key.len() != 64 {
        return Err(unreadable());
    }
    Ok(Some(registry))
}

fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub fn save(state_root: &Path, registry: &RegistryFile) -> RuntimeResult<()> {
    let bytes = serde_json::to_vec_pretty(registry).map_err(|_| write_failed())?;
    let temporary = state_root.join(format!("{REGISTRY_FILE}.tmp"));
    let result = (|| -> std::io::Result<()> {
        let mut file = private_file(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, state_root.join(REGISTRY_FILE))?;
        #[cfg(unix)]
        File::open(state_root)?.sync_all()?;
        Ok(())
    })();
    result.map_err(|_| write_failed())
}
