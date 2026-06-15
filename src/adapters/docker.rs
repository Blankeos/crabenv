use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

use crate::models::{Project, Scope, SourceKind, VarSource, Workspace};

pub fn collect_dockerfile(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = workspace.root.join("Dockerfile");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path)?;
    let re = Regex::new(r#"^\s*(?:ENV|ARG)\s+([A-Z][A-Z0-9_]*)"#)?;
    let mut out = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if let Some(name) = re
            .captures(line)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string())
        {
            out.push(VarSource {
                name,
                owner: workspace.rel.clone(),
                scope: Scope::Unknown,
                kind: SourceKind::Docker,
                value_type: None,
                required: None,
                default_value: None,
                value: None,
                path: path.clone(),
                line: index + 1,
            });
        }
    }
    Ok(out)
}

pub fn collect_compose(project: &Project) -> Result<Vec<VarSource>> {
    let path = project.root.join("docker-compose.yml");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path)?;
    let re = Regex::new(r#"\$\{([A-Z][A-Z0-9_]*)"#)?;
    let mut out = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        for captures in re.captures_iter(line) {
            let name = captures.get(1).unwrap().as_str().to_string();
            out.push(VarSource {
                name,
                owner: PathBuf::from("."),
                scope: Scope::Unknown,
                kind: SourceKind::Docker,
                value_type: None,
                required: None,
                default_value: None,
                value: None,
                path: path.clone(),
                line: index + 1,
            });
        }
    }
    Ok(out)
}
