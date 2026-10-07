//! Pins the shell's security-relevant configuration. architecture-lint only
//! sees Cargo dependency names and `src/**/*.rs`; these files (capabilities,
//! tauri.conf.json and the absence of overlays, resolved Cargo features,
//! build.rs) and the resolved Tauri config are guarded here instead
//! (`mise run desktop:check`; local only, as no CI job builds the shell).

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
fn no_other_tauri_config_can_be_merged_in() {
    // tauri-build/codegen merge tauri.<platform>.conf.json (and the JSON5/TOML
    // forms) over tauri.conf.json; a Windows-only overlay would never be
    // parsed by a Linux build, so refuse every such file here.
    let mut names: Vec<_> = std::fs::read_dir(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("tauri") && (lower.contains(".conf") || lower.ends_with(".toml"))
        })
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["tauri.conf.json"]);
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

/// The configuration Tauri actually embeds for this target (tauri.conf.json
/// merged with any platform overlay), not just the text of one file.
#[test]
fn the_resolved_tauri_config_keeps_the_pinned_security_settings() {
    use tauri::utils::config::{CapabilityEntry, Csp, DisabledCspModificationKind, PatternKind};
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

/// The features Cargo actually resolves, for every target and manifest table.
#[test]
fn cargo_resolves_tauri_with_only_the_pinned_features() {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = json(std::str::from_utf8(&output.stdout).unwrap());
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == env!("CARGO_PKG_NAME"))
        .expect("the shell package");
    assert_eq!(package["features"], serde_json::json!({}));
    let tauri: Vec<_> = package["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dependency| dependency["name"] == "tauri")
        .collect();
    assert_eq!(tauri.len(), 1, "{tauri:?}");
    assert_eq!(tauri[0]["uses_default_features"], false);
    assert_eq!(
        tauri[0]["features"],
        serde_json::json!(["wry", "x11", "common-controls-v6", "custom-protocol"])
    );
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| {
            node["id"]
                .as_str()
                .is_some_and(|id| id.ends_with("#tauri@2.12.1"))
        })
        .expect("resolved tauri");
    let mut features: Vec<_> = node["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature.as_str().unwrap())
        .collect();
    features.sort_unstable();
    assert_eq!(
        features,
        [
            "common-controls-v6",
            "custom-protocol",
            "tauri-runtime-wry",
            "webkit2gtk",
            "webview2-com",
            "wry",
            "x11"
        ]
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
