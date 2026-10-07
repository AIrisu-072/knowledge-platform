//! TEST-ONLY host for browser E2E: runs the real broker behind the same JSON
//! wire dispatcher the desktop shell uses, over stdin/stdout JSON lines.
//!
//! It is not the desktop shell, not a WebView and not shipped. It opens no
//! network listener. The "picker" answers from a queue fed by control lines,
//! standing in for the native dialog (an absent answer means cancel).
//!
//! Input lines:  {"id":1,"command":"workspace.list","request":null}
//!               {"id":2,"control":"pick","path":"/abs/folder"}   (or null)
//! Output lines: {"id":1,"ok":<value>} | {"id":1,"err":{"code":..,"reason":..}}

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use local_workspace_runtime::LocalWorkspaceRuntime;
use local_workspace_runtime::wire::{self, DirectoryPicker, PickerOutcome};
use serde_json::{Value, json};

struct QueuedPicker(RefCell<VecDeque<Option<PathBuf>>>);

impl DirectoryPicker for QueuedPicker {
    fn available(&self) -> bool {
        true
    }
    fn pick_directory(&self) -> PickerOutcome {
        match self.0.borrow_mut().pop_front().flatten() {
            Some(path) => PickerOutcome::Chosen(path),
            None => PickerOutcome::Cancelled,
        }
    }
}

fn main() {
    let state_root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("state root argument");
    let runtime = LocalWorkspaceRuntime::open(&state_root);
    let picker = QueuedPicker(RefCell::new(VecDeque::new()));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let reply = if message.get("control").and_then(Value::as_str) == Some("pick") {
            let path = message
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            picker.0.borrow_mut().push_back(path);
            json!({"id": id, "ok": null})
        } else {
            let command = message
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let request = message.get("request").cloned().unwrap_or(Value::Null);
            match wire::dispatch(
                runtime.as_ref().map_err(Clone::clone),
                &picker,
                command,
                request,
            ) {
                Ok(value) => json!({"id": id, "ok": value}),
                Err(error) => json!({"id": id, "err": error}),
            }
        };
        if writeln!(stdout, "{reply}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}
