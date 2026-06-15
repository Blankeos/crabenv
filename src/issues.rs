use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::adapters::{dotenv, typescript};
use crate::discovery::app_workspaces;
use crate::graph::EnvGraph;
use crate::models::{EnvSurface, Fix, Issue, Project, Severity, WorkspaceKind};
use crate::util::display_rel;

pub fn collect_issues(project: &Project, graph: &EnvGraph) -> Result<Vec<Issue>> {
    let mut issues = Vec::new();

    if project.is_monorepo {
        if !project.root.join(".env").exists() {
            issues.push(Issue {
                severity: Severity::Info,
                message: "root .env is missing; run crabenv copy to create it".to_string(),
                fix: Some(Fix::CreateLocalEnv),
            });
        }
        for workspace in app_workspaces(project) {
            if workspace.root.join(".env").exists() {
                issues.push(Issue {
                    severity: Severity::Warn,
                    message: format!(
                        "{} has a local .env; monorepos should use one root .env",
                        display_rel(&workspace.rel)
                    ),
                    fix: None,
                });
            }
        }
    } else if !project.root.join(".env").exists() {
        issues.push(Issue {
            severity: Severity::Info,
            message: ".env is missing; run crabenv copy to create it".to_string(),
            fix: Some(Fix::CreateLocalEnv),
        });
    }

    for workspace in &project.workspaces {
        if workspace.kind == WorkspaceKind::Package {
            if crate::adapters::workspace_has_owned_env_files(workspace) {
                issues.push(Issue {
                    severity: Severity::Error,
                    message: format!(
                        "{} is a package but owns env files",
                        display_rel(&workspace.rel)
                    ),
                    fix: None,
                });
            }
            continue;
        }

        let schema_vars = workspace_schema_vars(&workspace.rel, graph);
        let example_vars = dotenv::key_set(&dotenv::example_path(workspace))?;

        for name in schema_vars.difference(&example_vars) {
            issues.push(Issue {
                severity: Severity::Warn,
                message: format!(
                    "{} is in schema for {} but missing from template (.env.example)",
                    name,
                    display_rel(&workspace.rel)
                ),
                fix: Some(Fix::BackfillExample {
                    app: workspace.rel.clone(),
                    name: name.clone(),
                }),
            });
        }

        for name in example_vars.difference(&schema_vars) {
            issues.push(Issue {
                severity: Severity::Warn,
                message: format!(
                    "{} is in template (.env.example) for {} but missing from schema",
                    name,
                    display_rel(&workspace.rel)
                ),
                fix: None,
            });
        }

        let public_ts = typescript::public_schema_path(workspace);
        if public_ts.exists() {
            typescript::check_public_runtime_strict(&public_ts, &workspace.rel, &mut issues)?;
        }

        let package_json = workspace.root.join("package.json");
        if package_json.exists()
            && workspace.framework != "cloudflare-worker"
            && project.is_monorepo
            && !package_json_has_script(&package_json, "with-env")?
        {
            issues.push(Issue {
                severity: Severity::Warn,
                message: format!(
                    "{} package.json is missing a with-env script",
                    display_rel(&workspace.rel)
                ),
                fix: None,
            });
        }
    }

    for record in graph.values() {
        if record.example_value.is_some() && record.required == Some(true) && !record.local_present
        {
            issues.push(Issue {
                severity: Severity::Warn,
                message: format!(
                    "{} is required in {} but missing from {}",
                    record.name,
                    display_rel(&record.owner),
                    local_env_label(project)
                ),
                fix: Some(Fix::CreateLocalEnv),
            });
        }
    }

    for record in graph.values() {
        let has_definition_surface = record.surfaces.contains(&EnvSurface::Schema)
            || record.surfaces.contains(&EnvSurface::Template);
        let matched_elsewhere = graph.values().any(|other| {
            other.name == record.name
                && other.owner != record.owner
                && (other.surfaces.contains(&EnvSurface::Schema)
                    || other.surfaces.contains(&EnvSurface::Template))
        });
        if !has_definition_surface && record.surfaces.contains(&EnvSurface::Local) {
            if matched_elsewhere {
                continue;
            }
            issues.push(Issue {
                severity: Severity::Warn,
                message: format!(
                    "{} exists in local but is missing from schema and template",
                    record.name
                ),
                fix: None,
            });
        }
    }

    Ok(issues)
}

pub fn local_env_label(project: &Project) -> &'static str {
    if project.is_monorepo {
        "root .env"
    } else {
        ".env"
    }
}

fn workspace_schema_vars(owner: &Path, graph: &EnvGraph) -> BTreeSet<String> {
    graph
        .values()
        .filter(|record| record.owner == owner)
        .filter(|record| record.surfaces.contains(&EnvSurface::Schema))
        .map(|record| record.name.clone())
        .collect()
}

fn package_json_has_script(path: &Path, script: &str) -> Result<bool> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(value
        .get("scripts")
        .and_then(|scripts| scripts.get(script))
        .is_some())
}
