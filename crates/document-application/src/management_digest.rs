use document_domain::{PolicyMode, PolicyTarget, ResourceRef};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{ApplicationError, ManagementCommand, VerifiedActorContext};

const DIGEST_PREFIX: &[u8] = b"document-management-basics-v0\0";
const MAX_REASON_BYTES: usize = 1024;
const MAX_METADATA_PATCH_BYTES: usize = 64 * 1024;
const MAX_EXTENSIONS_DEPTH: usize = 16;

pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, ApplicationError> {
    let mut output = Vec::new();
    write_canonical_json(value, &mut output)?;
    Ok(output)
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), ApplicationError> {
    match value {
        Value::Object(map) => {
            output.push(b'{');
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                output.extend(serde_json::to_vec(key).map_err(json_error)?);
                output.push(b':');
                write_canonical_json(value, output)?;
            }
            output.push(b'}');
        }
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(item, output)?;
            }
            output.push(b']');
        }
        _ => output.extend(serde_json::to_vec(value).map_err(json_error)?),
    }
    Ok(())
}

fn json_error(error: serde_json::Error) -> ApplicationError {
    ApplicationError::Validation(format!("invalid management JSON: {error}"))
}

fn resource_json(resource: ResourceRef) -> Value {
    match resource {
        ResourceRef::Document(id) => json!({"type": "document", "id": id.as_uuid().to_string()}),
        ResourceRef::Folder(id) => json!({"type": "folder", "id": id.as_uuid().to_string()}),
        ResourceRef::AccessPolicy(id) => {
            json!({"type": "access_policy", "id": id.as_uuid().to_string()})
        }
    }
}

fn target_json(target: PolicyTarget) -> Value {
    resource_json(target.into())
}

fn validate_reason(reason: &str) -> Result<&str, ApplicationError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > MAX_REASON_BYTES || reason.chars().any(char::is_control)
    {
        return Err(ApplicationError::Validation(
            "reason must contain 1 to 1024 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(reason)
}

fn check_depth(value: &Value, depth: usize) -> bool {
    if depth > MAX_EXTENSIONS_DEPTH {
        return false;
    }
    match value {
        Value::Object(map) => map.values().all(|value| check_depth(value, depth + 1)),
        Value::Array(items) => items.iter().all(|value| check_depth(value, depth + 1)),
        _ => true,
    }
}

fn validate_metadata_patch(
    set: &std::collections::BTreeMap<String, Value>,
    unset: &std::collections::BTreeSet<String>,
) -> Result<(), ApplicationError> {
    const ALLOWED: [&str; 4] = [
        "document_type",
        "owning_department",
        "category",
        "extensions",
    ];
    for (key, value) in set {
        if !ALLOWED.contains(&key.as_str()) || unset.contains(key) {
            return Err(ApplicationError::Validation(
                "invalid metadata patch key".into(),
            ));
        }
        if key == "extensions" {
            if !value.is_object() || !check_depth(value, 0) {
                return Err(ApplicationError::Validation(
                    "invalid extensions object".into(),
                ));
            }
        } else if !value.is_string() {
            return Err(ApplicationError::Validation(
                "metadata value must be a string".into(),
            ));
        }
    }
    if unset.iter().any(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(ApplicationError::Validation(
            "invalid metadata patch key".into(),
        ));
    }
    let patch = json!({"set": set, "unset": unset});
    if canonical_json_bytes(&patch)?.len() > MAX_METADATA_PATCH_BYTES {
        return Err(ApplicationError::Validation(
            "metadata patch exceeds 64 KiB".into(),
        ));
    }
    Ok(())
}

fn command_payload(command: &ManagementCommand) -> Result<Value, ApplicationError> {
    let payload = match command {
        ManagementCommand::UpdateDocumentMetadata { set, unset, .. } => {
            validate_metadata_patch(set, unset)?;
            json!({"set": set, "unset": unset})
        }
        ManagementCommand::MoveDocument {
            from_folder_id,
            to_folder_id,
            ..
        } => {
            json!({"from_folder_id": from_folder_id.as_uuid().to_string(), "to_folder_id": to_folder_id.as_uuid().to_string()})
        }
        ManagementCommand::CreateFolder {
            parent_folder_id,
            name,
            ..
        } => {
            json!({"parent_folder_id": parent_folder_id.as_uuid().to_string(), "name": name})
        }
        ManagementCommand::RenameFolder { name, .. } => json!({"name": name}),
        ManagementCommand::MoveFolder {
            from_parent_id,
            to_parent_id,
            ..
        } => {
            json!({"from_parent_id": from_parent_id.as_uuid().to_string(), "to_parent_id": to_parent_id.as_uuid().to_string()})
        }
        ManagementCommand::SetAccessPolicy { target, mode, .. } => {
            mode.validate()?;
            let mode_value = match mode {
                PolicyMode::Inherit => json!({"kind": "inherit"}),
                PolicyMode::Explicit(grants) => {
                    let mut sorted = grants.clone();
                    sorted.sort_by(|a, b| a.subject().cmp(b.subject()));
                    json!({"kind": "explicit", "grants": sorted})
                }
            };
            json!({"target": target_json(*target), "mode": mode_value})
        }
    };
    Ok(payload)
}

pub fn canonical_command_bytes(
    ctx: &VerifiedActorContext,
    command: &ManagementCommand,
) -> Result<Vec<u8>, ApplicationError> {
    ctx.ensure_current()?;
    if command.expected_revision() < 0 {
        return Err(ApplicationError::Validation(
            "expected revision must be nonnegative".into(),
        ));
    }
    let reason = validate_reason(command.reason())?;
    let identity = json!({
        "schema_version": 1,
        "operation_id": command.operation_id().as_uuid().to_string(),
        "operation_kind": command.operation_kind(),
        "resource": resource_json(command.resource()),
        "expected_revision": command.expected_revision(),
        "payload": command_payload(command)?,
        "actor": {
            "identity_provider": ctx.principal().identity_provider(),
            "principal_id": ctx.principal().principal_id(),
        },
        "invocation_kind": ctx.invocation_kind().as_str(),
        "reason": reason,
    });
    canonical_json_bytes(&identity)
}

pub fn management_command_digest(
    ctx: &VerifiedActorContext,
    command: &ManagementCommand,
) -> Result<[u8; 32], ApplicationError> {
    let bytes = canonical_command_bytes(ctx, command)?;
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_PREFIX);
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}
