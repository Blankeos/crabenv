use anyhow::{anyhow, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::models::{Scope, SourceKind, VarMutation, VarSource, Workspace};

pub fn collect_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let Some(path) = find_env_file(&workspace.root) else {
        return Ok(Vec::new());
    };
    schema_sources(&path, &workspace.rel)
}

pub fn schema_sources(path: &Path, owner: &Path) -> Result<Vec<VarSource>> {
    let contents = fs::read_to_string(path)?;
    let alias_re = Regex::new(r#"alias\s*=\s*"([A-Z][A-Z0-9_]*)""#)?;
    let default_re = Regex::new(r#"default\s*=\s*([^,\)]+)"#)?;
    let mut out = Vec::new();
    let mut pending_comments = Vec::<String>::new();
    for (index, line) in contents.lines().enumerate() {
        let Some(alias) = alias_re
            .captures(line)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string())
        else {
            let trimmed = line.trim_start();
            if let Some(comment) = trimmed.strip_prefix('#') {
                pending_comments.push(comment.trim().to_string());
            } else if trimmed.is_empty() {
                if !pending_comments.is_empty() {
                    pending_comments.clear();
                }
            } else {
                pending_comments.clear();
            }
            continue;
        };
        let default_value = default_re
            .captures(line)
            .and_then(|captures| captures.get(1))
            .map(|m| {
                m.as_str()
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string()
            });
        let description = normalize_python_description(&mut pending_comments);
        out.push(VarSource {
            name: alias,
            owner: owner.to_path_buf(),
            scope: Scope::Private,
            kind: SourceKind::PythonSchema,
            value_type: Some("string".to_string()),
            required: Some(default_value.is_none()),
            default_value,
            description,
            value: None,
            path: path.to_path_buf(),
            line: index + 1,
        });
    }
    Ok(out)
}

fn normalize_python_description(pending_comments: &mut Vec<String>) -> Option<String> {
    let parts = std::mem::take(pending_comments)
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let description = parts.join(" ");
    (!description.is_empty()).then_some(description)
}

pub fn find_env_file(root: &Path) -> Option<PathBuf> {
    let src = root.join("src");
    if !src.exists() {
        return None;
    }
    WalkDir::new(src)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("env.py"))
}

pub fn upsert_schema(app: &Workspace, mutation: &VarMutation) -> Result<()> {
    let path = find_env_file(&app.root).ok_or_else(|| anyhow!("python env.py not found"))?;
    remove_schema(app, &mutation.variable)?;
    let mut contents = fs::read_to_string(&path)?;
    let field_name = mutation.variable.to_lowercase();
    let field = if let Some(default_value) = mutation
        .default_value
        .as_ref()
        .or(mutation.example.as_ref())
    {
        format!(
            "    {field_name}: str = Field(default={default_value:?}, alias={:?})\n",
            mutation.variable
        )
    } else {
        format!(
            "    {field_name}: str = Field(alias={:?})\n",
            mutation.variable
        )
    };
    if let Some(index) = contents.find("    model_config") {
        contents.insert_str(index, &field);
    } else {
        contents.push_str(&field);
    }
    fs::write(path, contents)?;
    Ok(())
}

pub fn remove_schema(app: &Workspace, variable: &str) -> Result<()> {
    let Some(path) = find_env_file(&app.root) else {
        return Ok(());
    };
    let contents = fs::read_to_string(&path)?;
    let lines = contents
        .lines()
        .filter(|line| !line.contains(&format!("alias=\"{variable}\"")))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}
