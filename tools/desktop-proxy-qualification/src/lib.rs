//! Source-level proxy qualification only; no Tauri/WebView is instantiated.
extern crate self as tauri;
pub use http;
pub mod async_runtime {
    pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }
}
// Compile the real production file. Do not copy its implementation here.
#[path = "../../../apps/desktop/src-tauri/src/proxy.rs"]
pub mod proxy;
