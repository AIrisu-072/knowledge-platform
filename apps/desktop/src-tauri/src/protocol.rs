//! The app's own `tauri` scheme: embedded production assets plus the `/v1`
//! forwarder. Registering it replaces Tauri's default asset handler, whose
//! index.html fallback would otherwise answer `/v1/...` with HTML.

use std::sync::Arc;

use tauri::http::{HeaderValue, Method, Request, Response, StatusCode, header};
use tauri::{AppHandle, UriSchemeContext, UriSchemeResponder, Wry};

use crate::proxy::{self, Proxy};

/// Origin of the bundled page (Tauri's `tauri` scheme convention).
#[cfg(any(target_os = "windows", target_os = "android"))]
pub const APP_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(any(target_os = "windows", target_os = "android")))]
pub const APP_ORIGIN: &str = "tauri://localhost";

/// Only the single main window is served; nothing else may use the scheme.
fn serves(webview_label: &str) -> bool {
    webview_label == crate::MAIN_WINDOW
}

pub fn handle(
    proxy: &Arc<Proxy>,
    context: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    if !serves(context.webview_label()) {
        responder.respond(proxy::problem(
            StatusCode::FORBIDDEN,
            "利用できません。",
            APP_ORIGIN,
        ));
        return;
    }
    if proxy::is_api_path(request.uri().path()) {
        let proxy = Arc::clone(proxy);
        tauri::async_runtime::spawn(async move { responder.respond(proxy.forward(request).await) });
        return;
    }
    let app = context.app_handle().clone();
    tauri::async_runtime::spawn(async move { responder.respond(asset(&app, &request)) });
}

fn asset(app: &AppHandle<Wry>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return proxy::problem(
            StatusCode::METHOD_NOT_ALLOWED,
            "この操作は利用できません。",
            APP_ORIGIN,
        );
    }
    // Same resolution as Tauri's default handler (SPA fallback to index.html
    // and the configured CSP header for HTML).
    let Some(asset) = app.asset_resolver().get(request.uri().path().to_owned()) else {
        return proxy::problem(StatusCode::NOT_FOUND, "見つかりません。", APP_ORIGIN);
    };
    let mut response = Response::new(if request.method() == Method::HEAD {
        Vec::new()
    } else {
        asset.bytes
    });
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&asset.mime_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Some(csp) = asset
        .csp_header
        .as_deref()
        .and_then(|value| HeaderValue::from_str(value).ok())
    {
        headers.insert(header::CONTENT_SECURITY_POLICY, csp);
    }
    proxy::secure_headers(headers, APP_ORIGIN);
    response
}

#[cfg(test)]
mod tests {
    use super::serves;

    #[test]
    fn only_the_main_window_is_served() {
        assert!(serves("main"));
        for label in ["", "Main", "main2", "popup", "main "] {
            assert!(!serves(label), "{label:?}");
        }
    }
}
