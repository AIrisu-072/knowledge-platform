//! Pins the shell's security-relevant configuration. architecture-lint only
//! sees Cargo dependency names and `src/**/*.rs`; these files (capabilities,
//! tauri.conf.json, Cargo features, build.rs) are guarded here instead
//! (`mise run desktop:check`, local; the owner chose no desktop CI job).

use serde_json::Value;

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("valid JSON")
}

#[test]
fn the_only_capability_grants_the_one_command_to_the_bundled_main_window() {
    let names: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["main.json"]);
    let capability = json(include_str!("../capabilities/main.json"));
    let object = capability.as_object().unwrap();
    let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "$schema",
            "description",
            "identifier",
            "local",
            "permissions",
            "windows"
        ]
    );
    assert_eq!(capability["identifier"], "main-window");
    assert_eq!(capability["local"], true);
    assert_eq!(capability["windows"], serde_json::json!(["main"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["allow-local-workspace-runtime"])
    );
}

#[test]
fn tauri_config_keeps_one_code_created_window_a_strict_csp_and_nothing_dangerous() {
    let text = include_str!("../tauri.conf.json");
    assert!(!text.to_ascii_lowercase().contains("dangerous"));
    let config = json(text);
    assert!(config["build"].get("devUrl").is_none());
    assert!(config["build"].get("beforeDevCommand").is_none());
    assert!(
        config
            .get("plugins")
            .is_none_or(|plugins| plugins.as_object().is_some_and(|map| map.is_empty()))
    );
    let app = &config["app"];
    assert_eq!(app["withGlobalTauri"], true);
    assert_eq!(app["windows"], serde_json::json!([]));
    assert_eq!(
        app["security"]["capabilities"],
        serde_json::json!(["main-window"])
    );
    assert!(app["security"].get("assetProtocol").is_none());
    assert!(app["security"].get("pattern").is_none());
    assert_eq!(
        app["security"]["csp"],
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; frame-src 'none'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
    );
}

#[test]
fn cargo_enables_no_devtools_asset_protocol_or_plugin() {
    let manifest = include_str!("../Cargo.toml");
    let tauri = manifest
        .lines()
        .find(|line| line.starts_with("tauri = "))
        .expect("tauri dependency");
    assert!(
        tauri.contains(r#"features = ["wry", "x11", "common-controls-v6", "custom-protocol"]"#),
        "{tauri}"
    );
    assert!(tauri.contains("default-features = false"));
    assert!(!manifest.contains("tauri-plugin"));
    let build = include_str!("../build.rs");
    for forbidden in ["std::process", "Command::new", "std::fs"] {
        assert!(!build.contains(forbidden), "{forbidden}");
    }
}
