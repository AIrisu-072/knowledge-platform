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

use std::path::{Path, PathBuf};
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

/// Only the bundled app's own URLs may load in the main window: exactly the
/// app origin (scheme, host and default port), without userinfo. On Windows
/// WebView2 intercepts only `http://tauri.*`, so `https`, another port or
/// userinfo would reach the network instead of the app.
fn is_app_url(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && proxy::serialized_origin(url).as_deref() == Some(protocol::APP_ORIGIN)
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

/// Where an allowed download is saved. The WebView's proposal is kept when it
/// is already inside Downloads; otherwise (for example WebKitGTK falling back
/// to the working directory when no XDG download dir is set) the same file
/// name is placed in Downloads, de-duplicated as `name (n).ext` like the
/// WebView does. `None` cancels the download.
fn download_target(
    url: &Url,
    proposed: &Path,
    downloads: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let dir = downloads?;
    if allow_download(url, proposed, Some(dir)) {
        return Some(proposed.to_path_buf());
    }
    if !is_app_blob(url) {
        return None;
    }
    let name = proposed.file_name()?.to_str()?;
    let (base, ext) = name
        .split_once('.')
        .map_or((name, String::new()), |(base, ext)| {
            (base, format!(".{ext}"))
        });
    let mut candidate = dir.join(name);
    let mut counter = 1;
    while exists(&candidate) {
        candidate = dir.join(format!("{base} ({counter}){ext}"));
        counter += 1;
    }
    Some(candidate)
}

/// The user's Downloads folder; `~/Downloads` when the platform has none
/// configured (it must already exist; nothing is created).
fn downloads_dir(app: &tauri::App) -> Option<PathBuf> {
    app.path().download_dir().ok().or_else(|| {
        app.path()
            .home_dir()
            .ok()
            .map(|home| home.join("Downloads"))
            .filter(|dir| dir.is_dir())
    })
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
            let downloads = downloads_dir(app);
            WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("/".into()))
                .title("Knowledge Platform")
                .inner_size(1280.0, 800.0)
                .min_inner_size(960.0, 600.0)
                .initialization_script(TRANSPORT_SHIM)
                .on_navigation(may_navigate)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(move |_, event| match event {
                    DownloadEvent::Requested { url, destination } => {
                        match download_target(&url, destination, downloads.as_deref(), Path::exists)
                        {
                            Some(target) => {
                                *destination = target;
                                true
                            }
                            None => false,
                        }
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

    /// The configuration Tauri actually embeds for this target (tauri.conf.json
    /// merged with any platform overlay), not just the text of one file.
    #[test]
    fn the_resolved_tauri_config_keeps_the_pinned_security_settings() {
        use tauri::utils::config::{
            CapabilityEntry, Csp, DisabledCspModificationKind, PatternKind,
        };
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let config = context.config();
        let security = &config.app.security;
        assert!(
            matches!(&security.csp, Some(Csp::Policy(policy)) if policy == "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"),
            "{:?}",
            security.csp
        );
        assert!(security.dev_csp.is_none());
        assert_eq!(
            security.dangerous_disable_asset_csp_modification,
            DisabledCspModificationKind::Flag(false)
        );
        assert!(!security.asset_protocol.enable);
        assert!(matches!(security.pattern, PatternKind::Brownfield));
        assert!(
            matches!(security.capabilities.as_slice(), [CapabilityEntry::Reference(name)] if name == "main-window")
        );
        assert!(security.headers.is_none());
        assert!(config.app.windows.is_empty());
        assert!(config.app.with_global_tauri);
        assert!(config.build.dev_url.is_none());
        assert!(config.plugins.0.is_empty());
    }

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
    fn only_the_exact_app_origin_counts_as_the_app() {
        // WebView2 intercepts only http://tauri.* (default scheme), so https,
        // other ports or userinfo would reach the network instead of the app.
        let app = format!("{}/documents", protocol::APP_ORIGIN);
        assert!(is_app_url(&Url::parse(&app).unwrap()));
        for url in [
            "https://tauri.localhost/",
            "https://tauri.localhost:8443/x",
            "http://tauri.localhost:1234/",
            "http://x@tauri.localhost/",
            "http://x:y@tauri.localhost/",
            "tauri://x@localhost/",
            "tauri://localhost:8080/",
        ] {
            assert!(!is_app_url(&Url::parse(url).unwrap()), "{url}");
        }
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
    fn downloads_go_into_the_downloads_folder_even_when_the_webview_proposed_elsewhere() {
        let downloads = Path::new("/home/u/Downloads");
        let blob = Url::parse(&format!("blob:{}/0b5c", protocol::APP_ORIGIN)).unwrap();
        let taken = |path: &Path| {
            path == Path::new("/home/u/Downloads/a.txt")
                || path == Path::new("/home/u/Downloads/a (1).txt")
        };
        // WebKit fell back to the working directory (no XDG download dir).
        assert_eq!(
            download_target(&blob, Path::new("/cwd/a.txt"), Some(downloads), taken),
            Some(downloads.join("a (2).txt"))
        );
        assert_eq!(
            download_target(&blob, Path::new("/cwd/b.tar.gz"), Some(downloads), taken),
            Some(downloads.join("b.tar.gz"))
        );
        // Already de-duplicated by the WebView inside Downloads: kept as is.
        assert_eq!(
            download_target(&blob, &downloads.join("a (1).txt"), Some(downloads), |_| {
                true
            }),
            Some(downloads.join("a (1).txt"))
        );
        assert_eq!(
            download_target(&blob, Path::new("/cwd/a.txt"), None, taken),
            None
        );
        let foreign = Url::parse("blob:https://evil.example/0b5c").unwrap();
        assert_eq!(
            download_target(&foreign, &downloads.join("a.txt"), Some(downloads), |_| {
                false
            }),
            None
        );
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
