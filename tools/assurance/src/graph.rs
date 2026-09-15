use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Repository,
    Component,
    File,
    Spec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    DependsOn,
    GovernedBy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
    files: BTreeSet<PathBuf>,
}

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("graph I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid cargo metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cargo metadata failed: {0}")]
    Cargo(String),
}

impl Graph {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn scan(root: &Path) -> Result<Self, GraphError> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(root)
            .output()?;
        if !output.status.success() {
            return Err(GraphError::Cargo(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Self::from_metadata_json(root, &String::from_utf8_lossy(&output.stdout))
    }

    pub fn from_metadata_json(root: &Path, metadata: &str) -> Result<Self, GraphError> {
        let mut graph = Self::default();
        graph.nodes.insert(
            "repository".to_string(),
            Node {
                id: "repository".to_string(),
                kind: NodeKind::Repository,
                path: Some(PathBuf::from(".")),
            },
        );
        collect_files(root, root, &mut graph)?;

        let metadata: Value = serde_json::from_str(metadata)?;
        let workspace = metadata
            .get("workspace_members")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let packages = metadata
            .get("packages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut package_by_name = BTreeMap::<String, String>::new();
        let mut dependencies = Vec::<(String, Vec<String>)>::new();

        for package in packages {
            let package_id = package
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !workspace.is_empty() && !workspace.contains(package_id) {
                continue;
            }
            let Some(manifest) = package.get("manifest_path").and_then(Value::as_str) else {
                continue;
            };
            let manifest = PathBuf::from(manifest);
            let component_path = manifest.parent().unwrap_or(root);
            let relative = component_path
                .strip_prefix(root)
                .unwrap_or(component_path)
                .to_path_buf();
            let component = normalize(&relative);
            let package_name = package
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&component)
                .to_string();

            graph.nodes.insert(
                component.clone(),
                Node {
                    id: component.clone(),
                    kind: NodeKind::Component,
                    path: Some(relative),
                },
            );
            graph.edges.push(Edge {
                from: "repository".to_string(),
                to: component.clone(),
                kind: EdgeKind::Contains,
            });
            package_by_name.insert(package_name, component.clone());

            let dependency_names = package
                .get("dependencies")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("name").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            dependencies.push((component, dependency_names));
        }

        for (from, dependency_names) in dependencies {
            for dependency_name in dependency_names {
                if let Some(to) = package_by_name.get(&dependency_name) {
                    graph.edges.push(Edge {
                        from: from.clone(),
                        to: to.clone(),
                        kind: EdgeKind::DependsOn,
                    });
                }
            }
        }

        Ok(graph)
    }

    pub fn component(&self, id: &str) -> Option<&Node> {
        self.nodes
            .get(id)
            .filter(|node| node.kind == NodeKind::Component)
    }

    pub fn files(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter()
    }

    pub fn depends_on(&self, from: &str, to: &str) -> bool {
        self.edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::DependsOn && edge.from == from && edge.to == to)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

fn collect_files(root: &Path, dir: &Path, graph: &mut Graph) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if matches!(name, ".git" | "target" | "node_modules" | ".worktrees") {
                continue;
            }
            collect_files(root, &path, graph)?;
            continue;
        }

        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let id = normalize(&relative);
        let kind = if id.starts_with("spec/") {
            NodeKind::Spec
        } else {
            NodeKind::File
        };
        graph.files.insert(relative.clone());
        graph.nodes.insert(
            id.clone(),
            Node {
                id: id.clone(),
                kind,
                path: Some(relative),
            },
        );
        graph.edges.push(Edge {
            from: "repository".to_string(),
            to: id,
            kind: EdgeKind::Contains,
        });
    }
    Ok(())
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
