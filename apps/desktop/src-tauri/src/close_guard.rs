//! Closing the window asks the page first, as closing a browser tab does.
//!
//! Without this, the window closes at once and the page's `beforeunload`
//! guards (unsaved drafts, operations whose result is not yet confirmed) never
//! run. On a close request the shell keeps the window open and asks WebKit to
//! close the page (`webkit_web_view_try_close`), which runs those guards and
//! shows the page's leave confirmation. Only when the page agrees does WebKit
//! emit `close`; wry then destroys the web view (its `close` handler), and the
//! window follows from the view's `destroy` signal. (A handler on `close`
//! itself would not run: wry's handler, connected first, destroys the view and
//! with it every later handler.)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{WebviewWindow, WindowEvent, Wry};
use webkit2gtk::WebViewExt;
use webkit2gtk::glib::prelude::ObjectExt;

pub fn install(window: &WebviewWindow<Wry>) -> tauri::Result<()> {
    let agreed = Arc::new(AtomicBool::new(false));
    {
        let window = window.clone();
        let agreed = Arc::clone(&agreed);
        window.clone().with_webview(move |webview| {
            webview.inner().connect_local("destroy", false, move |_| {
                agreed.store(true, Ordering::SeqCst);
                let _ = window.destroy();
                None
            });
        })?;
    }
    let target = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if agreed.load(Ordering::SeqCst) {
                return;
            }
            api.prevent_close();
            let _ = target.with_webview(|webview| webview.inner().try_close());
        }
    });
    Ok(())
}
