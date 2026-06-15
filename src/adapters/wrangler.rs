use anyhow::Result;
use regex::Regex;
use std::fs;

use crate::models::{Scope, SourceKind, VarSource, Workspace};

pub fn collect(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = workspace.root.join("wrangler.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path)?;
    let re = Regex::new(r#"^\s*([A-Z][A-Z0-9_]*)\s*="#)?;
    let mut in_vars = false;
    let mut out = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_vars = trimmed == "[vars]";
            continue;
        }
        if !in_vars {
            continue;
        }
        if let Some(name) = re
            .captures(line)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string())
        {
            out.push(VarSource {
                name,
                owner: workspace.rel.clone(),
                scope: Scope::Unknown,
                kind: SourceKind::Wrangler,
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
