use std::fs;
use std::path::{Path, PathBuf};

use crate::{Config, Error, Finding, Report};

pub fn check_repository(root: &Path, config: &Config) -> Result<Report, Error> {
    let mut findings = Vec::new();
    check_required_files(root, config, &mut findings);
    check_forbidden_top_level(root, config, &mut findings);
    check_workflows(root, config, &mut findings)?;
    check_workspace_boundaries(root, config, &mut findings)?;
    Ok(Report { findings })
}

fn check_required_files(root: &Path, config: &Config, findings: &mut Vec<Finding>) {
    for relative in &config.repository.required_files {
        if !root.join(relative).is_file() {
            findings.push(Finding {
                code: "ARCH_REQUIRED_FILE_MISSING".into(),
                path: relative.clone(),
                message: "required architecture/bootstrap file is missing".into(),
            });
        }
    }
}

fn check_forbidden_top_level(root: &Path, config: &Config, findings: &mut Vec<Finding>) {
    for relative in &config.repository.forbidden_top_level {
        if root.join(relative).exists() {
            findings.push(Finding {
                code: "ARCH_DOCKERFILE_VARIANT".into(),
                path: relative.clone(),
                message: "additional Dockerfile variants are forbidden; use the canonical root Dockerfile".into(),
            });
        }
    }
}

fn check_workflows(root: &Path, config: &Config, findings: &mut Vec<Finding>) -> Result<(), Error> {
    let workflow_dir = root.join(".github/workflows");
    if !workflow_dir.is_dir() {
        return Ok(());
    }

    for path in workflow_paths(&workflow_dir)? {
        let content = fs::read_to_string(&path)?;
        let relative = relative_path(root, &path);
        let lower = content.to_ascii_lowercase();

        if !config.ci.allow_self_hosted && runner_line_contains(&lower, "self-hosted") {
            findings.push(Finding {
                code: "ARCH_CI_SELF_HOSTED".into(),
                path: relative.clone(),
                message: "self-hosted runner is forbidden by architecture contract".into(),
            });
        }
        if !config.ci.allow_native_windows && runner_line_contains(&lower, "windows-") {
            findings.push(Finding {
                code: "ARCH_CI_NATIVE_WINDOWS".into(),
                path: relative.clone(),
                message:
                    "native Windows runner is forbidden; Windows development uses WSL2 semantics"
                        .into(),
            });
        }
        if config.ci.require_mise_entrypoint && !content.contains("mise run") {
            findings.push(Finding {
                code: "ARCH_CI_BYPASSES_MISE".into(),
                path: relative.clone(),
                message: "workflow must delegate project verification/build logic through mise run"
                    .into(),
            });
        }
        if config.ci.require_workflow_permissions && !has_top_level_permissions(&content) {
            findings.push(Finding {
                code: "ARCH_CI_PERMISSIONS_MISSING".into(),
                path: relative,
                message: "workflow must declare top-level permissions".into(),
            });
        }
    }
    Ok(())
}

fn check_workspace_boundaries(
    root: &Path,
    config: &Config,
    findings: &mut Vec<Finding>,
) -> Result<(), Error> {
    for rule in config.workspace.boundaries.values() {
        let crate_root = root.join(&rule.crate_path);
        if !crate_root.exists() {
            continue;
        }

        let manifest = crate_root.join("Cargo.toml");
        if manifest.is_file() {
            let content = fs::read_to_string(&manifest)?;
            let parsed: toml::Value = toml::from_str(&content)?;
            let dependency_tables = production_dependency_tables(&parsed);

            for forbidden in &rule.forbidden_dependencies {
                if dependency_tables
                    .iter()
                    .any(|table| dependency_table_contains(table, forbidden))
                {
                    findings.push(Finding {
                        code: "ARCH_FORBIDDEN_CRATE_DEPENDENCY".into(),
                        path: relative_path(root, &manifest),
                        message: format!(
                            "crate boundary forbids production dependency '{forbidden}'"
                        ),
                    });
                }
            }
        }

        let src = crate_root.join("src");
        if src.is_dir() {
            for path in rust_source_paths(&src)? {
                let content = fs::read_to_string(&path)?;
                for forbidden in &rule.forbidden_source_patterns {
                    if content.contains(forbidden) {
                        findings.push(Finding {
                            code: "ARCH_FORBIDDEN_SOURCE_PATTERN".into(),
                            path: relative_path(root, &path),
                            message: format!(
                                "crate boundary forbids source pattern '{forbidden}'"
                            ),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn production_dependency_tables(
    manifest: &toml::Value,
) -> Vec<&toml::map::Map<String, toml::Value>> {
    let mut tables = Vec::new();
    let Some(root) = manifest.as_table() else {
        return tables;
    };

    for name in ["dependencies", "build-dependencies"] {
        if let Some(table) = root.get(name).and_then(toml::Value::as_table) {
            tables.push(table);
        }
    }

    if let Some(targets) = root.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for name in ["dependencies", "build-dependencies"] {
                if let Some(table) = target.get(name).and_then(toml::Value::as_table) {
                    tables.push(table);
                }
            }
        }
    }

    tables
}

fn dependency_table_contains(
    table: &toml::map::Map<String, toml::Value>,
    forbidden: &str,
) -> bool {
    table.iter().any(|(key, value)| {
        if key == forbidden {
            return true;
        }
        value
            .as_table()
            .and_then(|spec| spec.get("package"))
            .and_then(toml::Value::as_str)
            .is_some_and(|package| package == forbidden)
    })
}

fn rust_source_paths(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    fn visit(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), Error> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, paths)?;
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                paths.push(path);
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn workflow_paths(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file()
            && matches!(
                path.extension().and_then(|v| v.to_str()),
                Some("yml" | "yaml")
            )
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn runner_line_contains(content: &str, needle: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("runs-on:") && trimmed.contains(needle)
    })
}

fn has_top_level_permissions(content: &str) -> bool {
    content.lines().any(|line| line.starts_with("permissions:"))
}
