use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters;
use crate::models::{Project, Workspace, WorkspaceKind};
use crate::util::{normalize_rel_display, normalize_rel_root};

pub fn discover_project(root: &Path) -> Result<Project> {
    let mut workspace_rels = BTreeSet::<PathBuf>::new();

    if let Some(patterns) = package_json_workspaces(&root.join("package.json"))? {
        for pattern in patterns {
            for rel in expand_workspace_pattern(root, &pattern)? {
                workspace_rels.insert(rel);
            }
        }
    }

    for pattern in pnpm_workspace_patterns(&root.join("pnpm-workspace.yaml"))? {
        for rel in expand_workspace_pattern(root, &pattern)? {
            workspace_rels.insert(rel);
        }
    }

    let apps_dir = root.join("apps");
    if apps_dir.is_dir() {
        for entry in fs::read_dir(&apps_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                workspace_rels.insert(PathBuf::from("apps").join(entry.file_name()));
            }
        }
    }

    let is_monorepo = !workspace_rels.is_empty();
    if !is_monorepo {
        workspace_rels.insert(PathBuf::from("."));
    }

    let mut workspaces = Vec::new();
    for rel in workspace_rels {
        let workspace_root = normalize_rel_root(root, &rel);
        if !workspace_root.is_dir() {
            continue;
        }

        let display_rel = normalize_rel_display(&rel);
        let mut workspace = Workspace {
            root: workspace_root,
            rel: display_rel,
            kind: WorkspaceKind::Package,
            framework: String::new(),
        };
        workspace.kind = classify_workspace(&workspace, &rel);
        workspace.framework = detect_framework(&workspace.root);
        workspaces.push(workspace);
    }

    Ok(Project {
        root: root.to_path_buf(),
        is_monorepo,
        workspaces,
    })
}

pub fn app_workspaces(project: &Project) -> impl Iterator<Item = &Workspace> {
    project
        .workspaces
        .iter()
        .filter(|workspace| workspace.kind == WorkspaceKind::App)
}

fn package_json_workspaces(path: &Path) -> Result<Option<Vec<String>>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("invalid package.json at {}", path.display()))?;
    let Some(workspaces) = value.get("workspaces") else {
        return Ok(None);
    };

    if let Some(items) = workspaces.as_array() {
        return Ok(Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect(),
        ));
    }

    if let Some(items) = workspaces.get("packages").and_then(Value::as_array) {
        return Ok(Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect(),
        ));
    }

    Ok(None)
}

fn pnpm_workspace_patterns(path: &Path) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&contents)
        .with_context(|| format!("invalid pnpm workspace file {}", path.display()))?;
    let mut patterns = Vec::new();
    if let Some(items) = value
        .get("packages")
        .and_then(serde_yaml::Value::as_sequence)
    {
        for item in items {
            if let Some(pattern) = item.as_str() {
                patterns.push(pattern.to_string());
            }
        }
    }
    Ok(patterns)
}

fn expand_workspace_pattern(root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.starts_with('!') {
        return Ok(Vec::new());
    }

    if !pattern.contains('*') {
        return Ok(vec![PathBuf::from(pattern)]);
    }

    let Some(star_index) = pattern.find('*') else {
        return Ok(vec![PathBuf::from(pattern)]);
    };
    let (base, suffix) = pattern.split_at(star_index);
    let suffix = suffix.trim_start_matches('*').trim_start_matches('/');
    let base_path = root.join(base.trim_end_matches('/'));
    if !base_path.is_dir() {
        return Ok(Vec::new());
    }

    let mut rels = Vec::new();
    for entry in fs::read_dir(&base_path)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = if suffix.is_empty() {
            entry.path()
        } else {
            entry.path().join(suffix)
        };
        if path.is_dir() {
            rels.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(rels)
}

fn classify_workspace(workspace: &Workspace, rel: &Path) -> WorkspaceKind {
    if rel == Path::new(".")
        || adapters::workspace_has_owned_env_files(workspace)
        || rel.starts_with("apps")
    {
        WorkspaceKind::App
    } else {
        WorkspaceKind::Package
    }
}

fn detect_framework(root: &Path) -> String {
    if root.join("wrangler.toml").exists() {
        return "cloudflare-worker".to_string();
    }
    if root.join("next.config.ts").exists() || root.join("next.config.js").exists() {
        return "nextjs".to_string();
    }
    if crate::adapters::python::find_env_file(root).is_some()
        || root.join("pyproject.toml").exists()
    {
        return "python".to_string();
    }
    if root.join("src/config.rs").exists() || root.join("Cargo.toml").exists() {
        return "rust".to_string();
    }
    if let Ok(package_json) = fs::read_to_string(root.join("package.json")) {
        if package_json.contains("\"hono\"") {
            return "hono".to_string();
        }
    }
    "typescript".to_string()
}
