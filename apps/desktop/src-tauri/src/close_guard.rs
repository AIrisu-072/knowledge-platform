//! Closing the window asks the page first, as closing a browser tab does.
//!
//! Without this, the window closes at once and the page's `beforeunload`
//! guards (unsaved drafts, operations whose result is not yet confirmed) never
//! run. On a close request the shell keeps the window open and asks WebKit to
//! close the page (`webkit_web_view_try_close`), which runs those guards.
//!
//! When a guard objects, WebKit hands the leave confirmation to the shell
//! (`script-dialog`), which asks in its own modal dialog ("このページに留まる"
//! is the default) and passes the answer back. Until the user answers, further
//! close requests are ignored: calling `try_close` again would restart WebKit's
//! 50 ms close timeout and close the window without the answer. When the page
//! agrees, WebKit emits `close`; wry then destroys the web view (its `close`
//! handler) and the window follows from the view's `destroy` signal. (A
//! handler on `close` itself would not run: wry's handler, connected first,
//! destroys the view and with it every later handler.)
//!
//! WebKit still closes the page without an answer when it does not reply to
//! the close request within 50 ms (a busy or hung page) or its web process has
//! gone, so a stuck page stays closable. Under WebDriver, WebKit's own prompt
//! is kept, which WebDriver accepts by itself.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use tauri::{WebviewWindow, WindowEvent, Wry};
use webkit2gtk::glib::prelude::ObjectExt;
use webkit2gtk::{ScriptDialogType, WebViewExt};

/// Title of the shell's leave confirmation (also how the GUI check finds it).
pub const CONFIRMATION_TITLE: &str = "閉じる前の確認";

/// A close request that produced neither a confirmation nor a close by then
/// (an unexpected state) may be retried.
const RETRY_AFTER: Duration = Duration::from_secs(3);

#[derive(Default)]
struct State {
    /// The page agreed and the window is being destroyed.
    agreed: bool,
    /// A `try_close` is in flight since this instant.
    deciding: Option<Instant>,
    /// The leave confirmation is open and waits for the user.
    asking: bool,
}

pub fn install(window: &WebviewWindow<Wry>) -> tauri::Result<()> {
    let state = Arc::new(Mutex::new(State::default()));
    {
        let window = window.clone();
        let state = Arc::clone(&state);
        window.clone().with_webview(move |webview| {
            let view = webview.inner();
            let destroyed = Arc::clone(&state);
            view.connect_local("destroy", false, move |_| {
                destroyed.lock().unwrap().agreed = true;
                let _ = window.destroy();
                None
            });
            view.connect_script_dialog(move |view, dialog| {
                if dialog.dialog_type() != ScriptDialogType::BeforeUnloadConfirm
                    || view.is_controlled_by_automation()
                {
                    return false;
                }
                state.lock().unwrap().asking = true;
                let confirmation = gtk::MessageDialog::builder()
                    .modal(true)
                    .message_type(gtk::MessageType::Question)
                    .title(CONFIRMATION_TITLE)
                    .text("このページを閉じますか？")
                    .secondary_text(
                        "保存していない入力や、結果を確認していない操作があります。閉じると、それらは失われます。",
                    )
                    .build();
                if let Some(parent) = view.toplevel().and_then(|top| top.downcast::<gtk::Window>().ok()) {
                    confirmation.set_transient_for(Some(&parent));
                }
                confirmation.set_position(gtk::WindowPosition::CenterOnParent);
                confirmation.add_button("このページに留まる(_S)", gtk::ResponseType::Cancel);
                confirmation.add_button("閉じる(_C)", gtk::ResponseType::Accept);
                confirmation.set_default_response(gtk::ResponseType::Cancel);
                let dialog = dialog.clone();
                let state = Arc::clone(&state);
                confirmation.connect_response(move |confirmation, response| {
                    let leave = response == gtk::ResponseType::Accept;
                    {
                        let mut state = state.lock().unwrap();
                        state.asking = false;
                        if !leave {
                            state.deciding = None;
                        }
                    }
                    dialog.confirm_set_confirmed(leave);
                    dialog.close();
                    // SAFETY: the dialog is owned by GTK and not used after this.
                    unsafe { confirmation.destroy() };
                });
                confirmation.show_all();
                true
            });
        })?;
    }
    let target = window.clone();
    window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        let mut state = state.lock().unwrap();
        if state.agreed {
            return;
        }
        api.prevent_close();
        // The user answers the open confirmation first; a request already
        // being decided is not repeated.
        if state.asking
            || state
                .deciding
                .is_some_and(|since| since.elapsed() < RETRY_AFTER)
        {
            return;
        }
        state.deciding = Some(Instant::now());
        drop(state);
        let _ = target.with_webview(|webview| webview.inner().try_close());
    });
    Ok(())
}
