use document_server::config::{Command, ConfigSource, RuntimeConfig};
use sqlx::{ConnectOptions, PgPool};
use std::{collections::BTreeMap, path::Path};
use tempfile::TempDir;

pub struct Environment(pub BTreeMap<String, String>);
impl ConfigSource for Environment {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}
pub struct Files {
    pub root: TempDir,
    pub environment: Environment,
}
impl Files {
    pub fn new(pool: &PgPool) -> Self {
        let root = tempfile::tempdir().unwrap();
        for directory in ["storage", "web", "web/assets"] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        std::fs::write(root.path().join("web/index.html"),"<!doctype html><html><head><script src=\"/assets/main.js\"></script><link rel=\"stylesheet\" href=\"/assets/main.css\"></head><body>synthetic-runtime-spa</body></html>").unwrap();
        std::fs::write(
            root.path().join("web/assets/main.js"),
            "// synthetic static test asset",
        )
        .unwrap();
        std::fs::write(
            root.path().join("web/assets/main.css"),
            "body { color: black; }",
        )
        .unwrap();
        // Executability-only probes, never used to claim production-worker acceptance.
        for worker in ["dsi", "diff"] {
            std::fs::write(root.path().join(worker), "#!/bin/sh\nexit 1\n").unwrap();
            Self::executable(&root.path().join(worker), true);
        }
        let environment = Environment(BTreeMap::from([
            ("KP_RUNTIME_MODE".into(), "poc".into()),
            ("KP_IDENTITY_PROFILE".into(), "poc-human".into()),
            (
                "KP_DATABASE_URL".into(),
                pool.connect_options().to_url_lossy().to_string(),
            ),
            ("KP_BIND".into(), "127.0.0.1:0".into()),
            (
                "KP_STORAGE_ROOT".into(),
                root.path().join("storage").display().to_string(),
            ),
            (
                "KP_DSI_WORKER".into(),
                root.path().join("dsi").display().to_string(),
            ),
            (
                "KP_DIFF_WORKER".into(),
                root.path().join("diff").display().to_string(),
            ),
            (
                "KP_WEB_DIST".into(),
                root.path().join("web").display().to_string(),
            ),
        ]));
        Self { root, environment }
    }
    pub fn config(&self) -> RuntimeConfig {
        RuntimeConfig::from_env(&self.environment, Command::Serve).unwrap()
    }
    pub fn executable(path: &Path, executable: bool) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
            )
            .unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = (path, executable);
        }
    }
}
