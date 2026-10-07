//! JSON IPC contract between the desktop shell and the frontend adapter.
//!
//! The shell exposes exactly one IPC command, `local_workspace_runtime`, with
//! `{ command, request }`. `command` is one of [`COMMANDS`]; every request is
//! a camelCase object that rejects unknown fields, so a caller can never pass
//! a path, privilege flag or extra option. Bytes travel as padded Base64.

use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    Capabilities, ContextRef, DirectorySelection, LocalRef, LocalWorkspaceRuntime,
    MAX_CREATE_BYTES, RuntimeError, RuntimeErrorCode, RuntimeErrorReason, base64,
};

/// The single IPC command name registered by the desktop shell.
pub const IPC_COMMAND: &str = "local_workspace_runtime";

/// Every command the frontend may send. Anything else is `unavailable`.
pub const COMMANDS: &[&str] = &[
    "capabilities",
    "workspace.list",
    "workspace.create",
    "workspace.recover",
    "workspace.rename",
    "directory.choose",
    "directory.attach",
    "directory.detach",
    "entries.list",
    "file.openRead",
    "file.read",
    "file.closeRead",
    "file.create",
];

pub enum PickerOutcome {
    Chosen(PathBuf),
    Cancelled,
    Failed,
}

/// Native single-folder picker owned by the shell (no file or multi select).
pub trait DirectoryPicker {
    fn available(&self) -> bool;
    fn pick_directory(&self) -> PickerOutcome;
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateWorkspace {
    name: String,
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recover {
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Rename {
    context: ContextRef,
    name: String,
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Choose {
    context: ContextRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Attach {
    context: ContextRef,
    selection_id: String,
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Detach {
    context: ContextRef,
    binding_id: String,
    operation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListEntries {
    context: ContextRef,
    r#ref: LocalRef,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenRead {
    context: ContextRef,
    r#ref: LocalRef,
    expected_file_identity: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadRange {
    context: ContextRef,
    read_handle_id: String,
    offset: u64,
    length: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseRead {
    context: ContextRef,
    read_handle_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateFile {
    context: ContextRef,
    parent: LocalRef,
    name: String,
    bytes_base64: String,
    operation_id: String,
}

fn invalid() -> RuntimeError {
    RuntimeError::with(
        RuntimeErrorCode::InvalidLocator,
        RuntimeErrorReason::InvalidRequest,
    )
}

fn parse<T: DeserializeOwned>(request: Value) -> Result<T, RuntimeError> {
    serde_json::from_value(request).map_err(|_| invalid())
}

fn json<T: serde::Serialize>(value: T) -> Result<Value, RuntimeError> {
    serde_json::to_value(value).map_err(|_| RuntimeError::new(RuntimeErrorCode::OutcomeUnknown))
}

/// Execute one IPC request. `runtime` is the result of opening the broker at
/// startup; when it failed every operation is `unavailable`.
pub fn dispatch(
    runtime: Result<&LocalWorkspaceRuntime, RuntimeError>,
    picker: &dyn DirectoryPicker,
    command: &str,
    request: Value,
) -> Result<Value, RuntimeError> {
    if !COMMANDS.contains(&command) {
        return Err(RuntimeError::with(
            RuntimeErrorCode::Unavailable,
            RuntimeErrorReason::UnknownCommand,
        ));
    }
    if command == "capabilities" {
        return json(match runtime {
            Ok(rt) => rt.capabilities(picker.available()),
            Err(_) => Capabilities::unavailable(),
        });
    }
    let rt = runtime?;
    match command {
        "workspace.list" => json(rt.list_workspaces()?),
        "workspace.create" => {
            let r: CreateWorkspace = parse(request)?;
            json(rt.create_workspace(&r.name, &r.operation_id)?)
        }
        "workspace.recover" => {
            let r: Recover = parse(request)?;
            json(rt.recover_workspace(&r.operation_id))
        }
        "workspace.rename" => {
            let r: Rename = parse(request)?;
            json(rt.rename_workspace(&r.context, &r.name, &r.operation_id)?)
        }
        "directory.choose" => {
            let r: Choose = parse(request)?;
            if !picker.available() {
                return Err(RuntimeError::with(
                    RuntimeErrorCode::Unavailable,
                    RuntimeErrorReason::UnsupportedPlatform,
                ));
            }
            // The broker lock is not held while the native dialog is open.
            let ticket = rt.begin_directory_selection(&r.context)?;
            match picker.pick_directory() {
                PickerOutcome::Chosen(path) => {
                    json(rt.finish_directory_selection(ticket, Some(path))?)
                }
                PickerOutcome::Cancelled => {
                    rt.finish_directory_selection(ticket, None)?;
                    Ok(Value::Null)
                }
                PickerOutcome::Failed => {
                    rt.finish_directory_selection(ticket, None)?;
                    Err(RuntimeError::with(
                        RuntimeErrorCode::Unavailable,
                        RuntimeErrorReason::Io,
                    ))
                }
            }
        }
        "directory.attach" => {
            let r: Attach = parse(request)?;
            let selection = DirectorySelection {
                selection_id: r.selection_id,
            };
            json(rt.attach_directory(&r.context, &selection, &r.operation_id)?)
        }
        "directory.detach" => {
            let r: Detach = parse(request)?;
            json(rt.detach_directory(&r.context, &r.binding_id, &r.operation_id)?)
        }
        "entries.list" => {
            let r: ListEntries = parse(request)?;
            json(rt.list_entries(&r.context, &r.r#ref, r.cursor.as_deref())?)
        }
        "file.openRead" => {
            let r: OpenRead = parse(request)?;
            json(rt.open_read(&r.context, &r.r#ref, &r.expected_file_identity)?)
        }
        "file.read" => {
            let r: ReadRange = parse(request)?;
            json(rt.read_file(&r.context, &r.read_handle_id, r.offset, r.length)?)
        }
        "file.closeRead" => {
            let r: CloseRead = parse(request)?;
            rt.close_read(&r.context, &r.read_handle_id)?;
            Ok(Value::Null)
        }
        "file.create" => {
            let r: CreateFile = parse(request)?;
            let bytes = base64::decode(&r.bytes_base64, MAX_CREATE_BYTES).map_err(|error| {
                if error.code == RuntimeErrorCode::Limit {
                    error
                } else {
                    invalid()
                }
            })?;
            json(rt.create_file(&r.context, &r.parent, &r.name, &bytes, &r.operation_id)?)
        }
        _ => Err(RuntimeError::with(
            RuntimeErrorCode::Unavailable,
            RuntimeErrorReason::UnknownCommand,
        )),
    }
}
