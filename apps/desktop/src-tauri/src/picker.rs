//! Native single-folder picker behind the broker's `DirectoryPicker`.
//!
//! The broker calls `pick_directory` from a blocking worker thread (never the
//! UI thread) without holding its lock. The chosen absolute path goes straight
//! to the broker; the page only ever receives an opaque selection ID.
//!
//! Threading: on Linux, rfd's GTK3 backend queues the dialog onto the default
//! GLib main context (the app's GTK main thread) and blocks the calling worker
//! until it closes, so calling it from the UI thread would deadlock. On
//! Windows the dialog runs on the calling thread and is parented to the main
//! window so it stays modal to the app.

use local_workspace_runtime::wire::{DirectoryPicker, PickerOutcome};
use tauri::{AppHandle, Wry};

pub const DIALOG_TITLE: &str = "追加するフォルダーを選択";

pub struct NativePicker {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    app: AppHandle<Wry>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    window_label: &'static str,
}

impl NativePicker {
    pub fn new(app: AppHandle<Wry>, window_label: &'static str) -> Self {
        Self { app, window_label }
    }
}

impl DirectoryPicker for NativePicker {
    fn available(&self) -> bool {
        cfg!(any(
            target_os = "linux",
            target_os = "windows",
            target_os = "macos"
        ))
    }

    fn pick_directory(&self) -> PickerOutcome {
        #[allow(unused_mut)]
        let mut dialog = rfd::FileDialog::new().set_title(DIALOG_TITLE);
        #[cfg(target_os = "windows")]
        if let Some(window) = tauri::Manager::get_webview_window(&self.app, self.window_label) {
            dialog = dialog.set_parent(&window);
        }
        match dialog.pick_folder() {
            Some(path) if path.is_absolute() => PickerOutcome::Chosen(path),
            Some(_) => PickerOutcome::Failed,
            None => PickerOutcome::Cancelled,
        }
    }
}
