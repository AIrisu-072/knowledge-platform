fn main() {
    // Only the committed tauri.conf.json may shape the shipped app. tauri-build
    // and generate_context! would merge a TAURI_CONFIG override on top (CSP,
    // windows, capabilities, asset protocol), so refuse to build with one.
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    assert!(
        std::env::var_os("TAURI_CONFIG").is_none(),
        "TAURI_CONFIG is not allowed: the desktop shell's configuration is pinned to tauri.conf.json"
    );
    // The single app command gets generated allow/deny permissions, so it is
    // reachable only where a capability grants it (capabilities/main.json).
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["local_workspace_runtime"])),
    )
    .expect("failed to run tauri-build");
}
