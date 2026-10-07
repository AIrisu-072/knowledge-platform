fn main() {
    // The single app command gets generated allow/deny permissions, so it is
    // reachable only where a capability grants it (capabilities/main.json).
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["local_workspace_runtime"])),
    )
    .expect("failed to run tauri-build");
}
