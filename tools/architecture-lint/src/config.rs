use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub version: u32,
    pub repository: RepositoryRules,
    pub platform: PlatformRules,
    pub ci: CiRules,
    pub workspace: WorkspaceRules,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepositoryRules {
    pub required_files: Vec<String>,
    pub forbidden_top_level: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformRules {
    pub production_target: String,
    pub native_windows_supported: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CiRules {
    pub allow_self_hosted: bool,
    pub allow_native_windows: bool,
    pub require_mise_entrypoint: bool,
    #[serde(default)]
    pub require_workflow_permissions: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRules {
    pub allowed_production_roots: Vec<String>,
    pub tooling_root: String,
    #[serde(default)]
    pub boundaries: BTreeMap<String, BoundaryRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoundaryRule {
    pub crate_path: String,
    #[serde(default)]
    pub forbidden_dependencies: Vec<String>,
    #[serde(default)]
    pub forbidden_source_patterns: Vec<String>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self, Error> {
        let path = root.join("spec/architecture/dependency-rules.toml");
        let content = fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn for_fixture() -> Self {
        Self {
            version: 1,
            repository: RepositoryRules {
                required_files: vec![
                    "spec/architecture/architecture-contract-v0.md".into(),
                    "spec/architecture/development-container-ci-architecture-v0.md".into(),
                    "spec/architecture/development-assurance-architecture-v0.md".into(),
                    "spec/operations/error-handling-resilience-requirements-v0.md".into(),
                    "spec/operations/observability-audit-requirements-v0.md".into(),
                    "spec/requirements/frontend-ux-requirements-v0.md".into(),
                    "spec/selection/library-tool-selection-v0.md".into(),
                    "mise.toml".into(),
                    "rust-toolchain.toml".into(),
                ],
                forbidden_top_level: vec![
                    "Dockerfile.dev".into(),
                    "Dockerfile.test".into(),
                    "Dockerfile.ci".into(),
                    "Dockerfile.prod".into(),
                ],
            },
            platform: PlatformRules {
                production_target: "linux/amd64".into(),
                native_windows_supported: false,
            },
            ci: CiRules {
                allow_self_hosted: false,
                allow_native_windows: false,
                require_mise_entrypoint: true,
                require_workflow_permissions: false,
            },
            workspace: WorkspaceRules {
                allowed_production_roots: vec!["crates".into(), "apps".into()],
                tooling_root: "tools".into(),
                boundaries: BTreeMap::from([
                    (
                        "document_domain".into(),
                        BoundaryRule {
                            crate_path: "crates/document-domain".into(),
                            forbidden_dependencies: vec![
                                "sqlx".into(),
                                "axum".into(),
                                "tokio".into(),
                            ],
                            forbidden_source_patterns: vec![
                                "std::fs".into(),
                                "std::path".into(),
                                "tokio::fs".into(),
                            ],
                        },
                    ),
                    (
                        "document_application".into(),
                        BoundaryRule {
                            crate_path: "crates/document-application".into(),
                            forbidden_dependencies: vec!["sqlx".into(), "axum".into()],
                            forbidden_source_patterns: vec![
                                "std::fs".into(),
                                "std::path".into(),
                                "tokio::fs".into(),
                            ],
                        },
                    ),
                ]),
            },
        }
    }
}
