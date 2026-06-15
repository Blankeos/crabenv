use anyhow::Result;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{DotenvEntry, Project, Scope, SourceKind, VarSource, Workspace};
use crate::util::is_valid_var_name;

pub fn example_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join(".env.example")
}

pub fn local_path(project: &Project, workspace: &Workspace) -> PathBuf {
    if project.is_monorepo {
        project.root.join(".env")
    } else {
        workspace.root.join(".env")
    }
}

pub fn collect_example(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = example_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &workspace.rel, SourceKind::EnvExample)
}

pub fn collect_local(project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
    if project.is_monorepo {
        return Ok(Vec::new());
    }

    let path = local_path(project, workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &workspace.rel, SourceKind::EnvLocal)
}

pub fn collect_root_local(project: &Project) -> Result<Vec<VarSource>> {
    if !project.is_monorepo {
        return Ok(Vec::new());
    }

    let path = project.root.join(".env");
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &PathBuf::from("."), SourceKind::EnvLocal)
}

pub fn sources(path: &Path, owner: &Path, kind: SourceKind) -> Result<Vec<VarSource>> {
    let contents = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if let Some((key, value)) = parse_line(line) {
            out.push(VarSource {
                name: key,
                owner: owner.to_path_buf(),
                scope: Scope::Unknown,
                kind: kind.clone(),
                value_type: None,
                required: None,
                default_value: None,
                value: Some(value),
                path: path.to_path_buf(),
                line: index + 1,
            });
        }
    }
    Ok(out)
}

pub fn parse_file(path: &Path) -> Result<Vec<DotenvEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    Ok(contents
        .lines()
        .filter_map(parse_line)
        .map(|(key, value)| DotenvEntry { key, value })
        .collect())
}

pub fn parse_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("export ") {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    let key = key.trim();
    if !is_valid_var_name(key) {
        return None;
    }
    Some((key.to_string(), unquote_value(value.trim())))
}

pub fn key_set(path: &Path) -> Result<BTreeSet<String>> {
    Ok(parse_file(path)?
        .into_iter()
        .map(|entry| entry.key)
        .collect())
}

pub fn upsert_example(path: &Path, key: &str, value: &str) -> Result<()> {
    let mut lines = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let replacement = format!("{key}={}", quote_value(value));
    let mut changed = false;
    for line in &mut lines {
        if parse_line(line)
            .map(|(line_key, _)| line_key == key)
            .unwrap_or(false)
        {
            *line = replacement.clone();
            changed = true;
        }
    }

    if !changed {
        if !lines.is_empty() && lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(replacement);
    }

    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

pub fn remove_key(path: &Path, key: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let lines = fs::read_to_string(path)?
        .lines()
        .filter(|line| {
            parse_line(line)
                .map(|(line_key, _)| line_key != key)
                .unwrap_or(true)
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    fs::write(path, format!("{}\n", lines.join("\n").trim_end()))?;
    Ok(())
}

pub fn quote_value(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '#' | '$' | '<' | '>'))
    {
        format!("{:?}", value)
    } else {
        value.to_string()
    }
}

fn unquote_value(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}
