//! Knowledge Platform desktop shell (Tauri v2).
//!
//! One main window shows the existing production web build. The page reaches
//! the local Workspace broker only through the single bounded IPC command
//! `local_workspace_runtime`, and the backend only through same-origin `/v1`
//! forwarding. No plugin, shell, process, general filesystem or multi-window
//! capability exists.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod picker;
mod protocol;
mod proxy;

use std::path::Path;
use std::sync::Arc;

use local_workspace_runtime::{
    LocalWorkspaceRuntime, RuntimeError, RuntimeErrorCode, RuntimeErrorReason, wire,
};
use serde_json::Value;
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{Manager, Url, WebviewUrl, WebviewWindowBuilder};

pub const MAIN_WINDOW: &str = "main";
const STATE_DIR: &str = "workspace-runtime";
/// Works around a WebKitGTK crash on Blob/FormData request bodies to the app
/// scheme (see the script). Runs before the bundled app in the main frame.
const TRANSPORT_SHIM: &str = include_str!("transport-shim.js");

struct Broker {
    runtime: Result<LocalWorkspaceRuntime, RuntimeError>,
    picker: picker::NativePicker,
}

/// The only app command. Requests and replies are the broker's wire JSON;
/// failures are its typed `{code, reason}` (never OS text or paths).
#[tauri::command]
async fn local_workspace_runtime(
    broker: tauri::State<'_, Arc<Broker>>,
    command: String,
    request: Value,
) -> Result<Value, Value> {
    let broker = Arc::clone(&broker);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        wire::dispatch(
            broker.runtime.as_ref().map_err(Clone::clone),
            &broker.picker,
            &command,
            request,
        )
    })
    .await;
    match outcome {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(serde_json::to_value(error).unwrap_or(Value::Null)),
        // A lost worker is untyped on purpose: the page treats it as unknown
        // for mutations and unavailable for reads.
        Err(_) => Err(Value::String("runtime worker failed".into())),
    }
}

/// Only the bundled app's own URLs may load in the main window.
fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => {
            cfg!(any(target_os = "windows", target_os = "android"))
                && url.host_str() == Some("tauri.localhost")
        }
        _ => false,
    }
}

/// A `blob:` URL minted by the bundled app itself.
fn is_app_blob(url: &Url) -> bool {
    url.scheme() == "blob" && Url::parse(url.path()).is_ok_and(|inner| is_app_url(&inner))
}

/// Main-window navigation: the bundled app, plus the app's own blob URLs,
/// which WebKit routes through the navigation policy before turning an
/// `<a download>` click into a download.
fn may_navigate(url: &Url) -> bool {
    is_app_url(url) || is_app_blob(url)
}

/// Downloads are the existing "save original" buttons: same-origin blob URLs
/// saved under the user's Downloads folder with the WebView's de-duplicated
/// file name. Anything else is cancelled.
fn allow_download(url: &Url, destination: &Path, downloads: Option<&Path>) -> bool {
    let inside = downloads
        .is_some_and(|dir| destination.parent() == Some(dir) && destination.file_name().is_some());
    is_app_blob(url) && inside
}

fn open_broker(app: &tauri::App) -> Result<LocalWorkspaceRuntime, RuntimeError> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| RuntimeError::with(RuntimeErrorCode::Unavailable, RuntimeErrorReason::Io))?;
    LocalWorkspaceRuntime::open(root.join(STATE_DIR))
}

fn main() {
    let proxy = Arc::new(proxy::Proxy::from_env(protocol::APP_ORIGIN));
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("tauri", move |context, request, responder| {
            protocol::handle(&proxy, context, request, responder);
        })
        .invoke_handler(tauri::generate_handler![local_workspace_runtime])
        .setup(|app| {
            let runtime = open_broker(app);
            let picker = picker::NativePicker::new(app.handle().clone(), MAIN_WINDOW);
            app.manage(Arc::new(Broker { runtime, picker }));
            let downloads = app.path().download_dir().ok();
            WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("/".into()))
                .title("Knowledge Platform")
                .inner_size(1280.0, 800.0)
                .min_inner_size(960.0, 600.0)
                .initialization_script(TRANSPORT_SHIM)
                .on_navigation(may_navigate)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(move |_, event| match event {
                    DownloadEvent::Requested { url, destination } => {
                        allow_download(&url, destination, downloads.as_deref())
                    }
                    _ => true,
                })
                .disable_drag_drop_handler()
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run the desktop shell");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bundled_app_urls_may_load() {
        assert!(is_app_url(
            &Url::parse("tauri://localhost/documents").unwrap()
        ));
        for url in [
            "https://example.com/",
            "tauri://evil/",
            "file:///etc/passwd",
            "data:text/html,x",
            "about:blank",
            "ipc://localhost/x",
        ] {
            assert!(!is_app_url(&Url::parse(url).unwrap()), "{url}");
        }
        assert_eq!(
            is_app_url(&Url::parse("http://tauri.localhost/").unwrap()),
            cfg!(target_os = "windows")
        );
    }

    #[test]
    fn navigation_allows_the_app_and_its_own_blob_downloads_only() {
        let own_blob = format!("blob:{}/0b5c", protocol::APP_ORIGIN);
        assert!(may_navigate(
            &Url::parse("tauri://localhost/tasks").unwrap()
        ));
        assert!(may_navigate(&Url::parse(&own_blob).unwrap()));
        for url in [
            "blob:https://evil.example/0b5c",
            "blob:null/0b5c",
            "https://example.com/",
            "data:text/html,x",
            "file:///etc/passwd",
        ] {
            assert!(!may_navigate(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn downloads_are_same_origin_blobs_into_the_downloads_folder() {
        let downloads = Path::new("/home/u/Downloads");
        let blob = Url::parse(&format!("blob:{}/0b5c", protocol::APP_ORIGIN)).unwrap();
        assert!(allow_download(
            &blob,
            &downloads.join("a.txt"),
            Some(downloads)
        ));
        assert!(!allow_download(
            &blob,
            &downloads.join("sub/a.txt"),
            Some(downloads)
        ));
        assert!(!allow_download(
            &blob,
            Path::new("/etc/a.txt"),
            Some(downloads)
        ));
        assert!(!allow_download(&blob, &downloads.join("a.txt"), None));
        for other in [
            "blob:https://evil.example/0b5c",
            "https://evil.example/a.txt",
            "data:text/plain,x",
        ] {
            assert!(
                !allow_download(
                    &Url::parse(other).unwrap(),
                    &downloads.join("a.txt"),
                    Some(downloads)
                ),
                "{other}"
            );
        }
    }
}
