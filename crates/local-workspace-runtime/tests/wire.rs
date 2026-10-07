#![cfg(unix)]

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

use local_workspace_runtime::wire::{self, DirectoryPicker, PickerOutcome};
use local_workspace_runtime::{LocalWorkspaceRuntime, RuntimeError, RuntimeErrorCode};
use serde_json::{Value, json};

struct FakePicker {
    next: RefCell<Vec<PickerOutcome>>,
    calls: RefCell<u32>,
}

impl FakePicker {
    fn new(outcomes: Vec<PickerOutcome>) -> Self {
        Self {
            next: RefCell::new(outcomes),
            calls: RefCell::new(0),
        }
    }
}

impl DirectoryPicker for FakePicker {
    fn available(&self) -> bool {
        true
    }
    fn pick_directory(&self) -> PickerOutcome {
        *self.calls.borrow_mut() += 1;
        self.next.borrow_mut().remove(0)
    }
}

fn call(
    rt: &LocalWorkspaceRuntime,
    picker: &FakePicker,
    command: &str,
    request: Value,
) -> Result<Value, RuntimeError> {
    wire::dispatch(Ok(rt), picker, command, request)
}

#[test]
fn full_round_trip_over_the_json_wire() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("folder");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("a.txt"), b"hello").unwrap();
    let rt = LocalWorkspaceRuntime::open(temp.path().join("state")).unwrap();
    let picker = FakePicker::new(vec![
        PickerOutcome::Cancelled,
        PickerOutcome::Chosen(folder.clone()),
    ]);

    let caps = call(&rt, &picker, "capabilities", Value::Null).unwrap();
    assert_eq!(caps["localResources"], "available");
    assert_eq!(caps["nativeDirectoryPicker"], "available");

    let created = call(
        &rt,
        &picker,
        "workspace.create",
        json!({"name": "案件A", "operationId": "op-create-1"}),
    )
    .unwrap();
    let workspace = &created["workspace"];
    let context = json!({
        "workspaceId": workspace["workspaceId"],
        "effectiveContextRevision": workspace["effectiveContextRevision"],
    });
    assert_eq!(created["receipt"]["operationId"], "op-create-1");
    assert_eq!(
        call(
            &rt,
            &picker,
            "workspace.recover",
            json!({"operationId": "op-create-1"})
        )
        .unwrap()["state"],
        "ready"
    );
    let listed = call(&rt, &picker, "workspace.list", Value::Null).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Cancel returns null and sends nothing else.
    assert_eq!(
        call(
            &rt,
            &picker,
            "directory.choose",
            json!({"context": context})
        )
        .unwrap(),
        Value::Null
    );
    let selection = call(
        &rt,
        &picker,
        "directory.choose",
        json!({"context": context}),
    )
    .unwrap();
    assert_eq!(*picker.calls.borrow(), 2);
    let attached = call(
        &rt,
        &picker,
        "directory.attach",
        json!({"context": context, "selectionId": selection["selectionId"], "operationId": "op-attach-1"}),
    )
    .unwrap();
    let binding = attached["receipt"]["bindingId"].clone();
    let workspace = &attached["workspace"];
    let context = json!({
        "workspaceId": workspace["workspaceId"],
        "effectiveContextRevision": workspace["effectiveContextRevision"],
    });

    let page = call(
        &rt,
        &picker,
        "entries.list",
        json!({"context": context, "ref": {"bindingId": binding, "locator": []}, "cursor": null}),
    )
    .unwrap();
    let entry = &page["entries"][0];
    assert_eq!(entry["name"], "a.txt");
    assert_eq!(page["omittedCount"], 0);
    let handle = call(
        &rt,
        &picker,
        "file.openRead",
        json!({"context": context, "ref": {"bindingId": binding, "locator": ["a.txt"]}, "expectedFileIdentity": entry["fileIdentity"]}),
    )
    .unwrap();
    let bytes = call(
        &rt,
        &picker,
        "file.read",
        json!({"context": context, "readHandleId": handle["readHandleId"], "offset": 0, "length": 1024}),
    )
    .unwrap();
    assert_eq!(bytes["bytesBase64"], "aGVsbG8=");
    assert_eq!(
        call(
            &rt,
            &picker,
            "file.closeRead",
            json!({"context": context, "readHandleId": handle["readHandleId"]}),
        )
        .unwrap(),
        Value::Null
    );
    let receipt = call(
        &rt,
        &picker,
        "file.create",
        json!({"context": context, "parent": {"bindingId": binding, "locator": []}, "name": "b.txt", "bytesBase64": "d29ybGQ=", "operationId": "op-file-1"}),
    )
    .unwrap();
    assert_eq!(receipt["ref"]["locator"], json!(["b.txt"]));
    assert_eq!(fs::read(folder.join("b.txt")).unwrap(), b"world");
    let renamed = call(
        &rt,
        &picker,
        "workspace.rename",
        json!({"context": context, "name": "案件B", "operationId": "op-rename-1"}),
    )
    .unwrap();
    assert_eq!(renamed["name"], "案件B");
    let detached = call(
        &rt,
        &picker,
        "directory.detach",
        json!({"context": context, "bindingId": binding, "operationId": "op-detach-1"}),
    )
    .unwrap();
    assert_eq!(detached["bindings"].as_array().unwrap().len(), 1);
}

#[test]
fn malformed_requests_and_unknown_commands_fail_with_safe_codes() {
    let temp = tempfile::tempdir().unwrap();
    let rt = LocalWorkspaceRuntime::open(temp.path().join("state")).unwrap();
    let picker = FakePicker::new(vec![]);
    for (command, request) in [
        ("workspace.create", json!({"name": "x"})),
        (
            "workspace.create",
            json!({"name": "x", "operationId": "o", "path": "/etc"}),
        ),
        (
            "entries.list",
            json!({"context": {}, "ref": {"bindingId": "b", "locator": "/"}}),
        ),
        (
            "file.create",
            json!({"context": {"workspaceId": "w", "effectiveContextRevision": "c0"}, "parent": {"bindingId": "b", "locator": []}, "name": "n", "bytesBase64": "***", "operationId": "o"}),
        ),
    ] {
        let error = call(&rt, &picker, command, request).unwrap_err();
        assert_eq!(error.code, RuntimeErrorCode::InvalidLocator, "{command}");
    }
    for command in ["shell.open", "fs.readFile", "process.spawn", ""] {
        assert_eq!(
            call(&rt, &picker, command, Value::Null).unwrap_err().code,
            RuntimeErrorCode::Unavailable,
            "{command}"
        );
    }
    let error =
        serde_json::to_value(call(&rt, &picker, "shell.open", Value::Null).unwrap_err()).unwrap();
    assert_eq!(
        error,
        json!({"code": "unavailable", "reason": "unknown_command"})
    );
}

#[test]
fn missing_runtime_reports_unavailable_capabilities_and_operations() {
    let picker = FakePicker::new(vec![]);
    let open_error = RuntimeError::new(RuntimeErrorCode::Unavailable);
    let caps = wire::dispatch(
        Err(open_error.clone()),
        &picker,
        "capabilities",
        Value::Null,
    )
    .unwrap();
    assert_eq!(caps["localResources"], "unavailable");
    assert_eq!(caps["managedWorkspace"], "unavailable");
    assert_eq!(
        wire::dispatch(Err(open_error), &picker, "workspace.list", Value::Null)
            .unwrap_err()
            .code,
        RuntimeErrorCode::Unavailable
    );
}

#[test]
fn picker_failure_is_reported_and_releases_the_picker() {
    let temp = tempfile::tempdir().unwrap();
    let rt = LocalWorkspaceRuntime::open(temp.path().join("state")).unwrap();
    let picker = FakePicker::new(vec![PickerOutcome::Failed, PickerOutcome::Cancelled]);
    let created = call(
        &rt,
        &picker,
        "workspace.create",
        json!({"name": "w", "operationId": "o1"}),
    )
    .unwrap();
    let w = &created["workspace"];
    let context = json!({"workspaceId": w["workspaceId"], "effectiveContextRevision": w["effectiveContextRevision"]});
    assert_eq!(
        call(
            &rt,
            &picker,
            "directory.choose",
            json!({"context": context})
        )
        .unwrap_err()
        .code,
        RuntimeErrorCode::Unavailable
    );
    assert_eq!(
        call(
            &rt,
            &picker,
            "directory.choose",
            json!({"context": context})
        )
        .unwrap(),
        Value::Null
    );
    let _ = PathBuf::new();
}

struct PanickingPicker;

impl DirectoryPicker for PanickingPicker {
    fn available(&self) -> bool {
        true
    }
    fn pick_directory(&self) -> PickerOutcome {
        panic!("native dialog failed");
    }
}

#[test]
fn a_panicking_native_picker_does_not_leave_the_picker_busy() {
    let temp = tempfile::tempdir().unwrap();
    let rt = LocalWorkspaceRuntime::open(temp.path().join("state")).unwrap();
    let ok = FakePicker::new(vec![PickerOutcome::Cancelled]);
    let created = call(
        &rt,
        &ok,
        "workspace.create",
        json!({"name": "w", "operationId": "o1"}),
    )
    .unwrap();
    let w = &created["workspace"];
    let context = json!({"workspaceId": w["workspaceId"], "effectiveContextRevision": w["effectiveContextRevision"]});
    let request = json!({"context": context});
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wire::dispatch(
            Ok(&rt),
            &PanickingPicker,
            "directory.choose",
            request.clone(),
        )
    }));
    assert!(outcome.is_err());
    assert_eq!(
        call(&rt, &ok, "directory.choose", request).unwrap(),
        Value::Null
    );
}
