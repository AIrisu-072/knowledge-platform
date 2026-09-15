use std::fs;
use std::path::{Path, PathBuf};

use crate::{Config, Error, Finding, Report};

pub fn check_repository(root: &Path, config: &Config) -> Result<Report, Error> {
    let mut findings = Vec::new();
    check_required_files(root, config, &mut findings);
    check_forbidden_top_level(root, config, &mut findings);
    check_workflows(root, config, &mut findings)?;
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
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
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

fn runner_line_contains(content: &str, needle: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("runs-on:") && trimmed.contains(needle)
    })
}

fn has_top_level_permissions(content: &str) -> bool {
    content.lines().any(|line| line.starts_with("permissions:"))
}
