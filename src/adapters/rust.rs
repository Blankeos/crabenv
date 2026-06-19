use anyhow::{anyhow, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{Scope, SourceKind, VarMutation, VarSource, Workspace};

pub fn config_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/config.rs")
}

pub fn collect_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = config_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    schema_sources(&path, &workspace.rel)
}

pub fn schema_sources(path: &Path, owner: &Path) -> Result<Vec<VarSource>> {
    let contents = fs::read_to_string(path)?;
    let rename_re = Regex::new(r#"rename\s*=\s*\"([A-Z][A-Z0-9_]*)\""#)?;
    let field_re = Regex::new(r#"pub\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*([^,]+),?"#)?;
    let default_re = Regex::new(r#"default(?:\s*=\s*\"([^\"]+)\")?"#)?;

    let mut out = Vec::new();
    let mut pending_attrs = Vec::<String>::new();
    let mut pending_comments = Vec::<String>::new();

    for (index, line) in contents.lines().enumerate() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("///") {
            pending_comments.push(trimmed.trim_start_matches("///").trim().to_string());
            continue;
        }

        if trimmed.starts_with("#[") {
            pending_attrs.push(trimmed.to_string());
            continue;
        }

        let Some(field_captures) = field_re.captures(trimmed) else {
            if trimmed.is_empty() {
                if !pending_attrs.is_empty() || !pending_comments.is_empty() {
                    pending_attrs.clear();
                    pending_comments.clear();
                }
            } else if !trimmed.starts_with("//") {
                pending_attrs.clear();
                pending_comments.clear();
            }
            continue;
        };

        let attrs = pending_attrs.join(" ");
        let Some(name) = rename_re
            .captures(&attrs)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string())
        else {
            pending_attrs.clear();
            pending_comments.clear();
            continue;
        };

        let ty = field_captures
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| "String".to_string());
        let is_optional = ty.starts_with("Option<") || ty.starts_with("Option <");
        let default_value = default_re
            .captures(&attrs)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string());
        let description = normalize_rust_description(&mut pending_comments);

        out.push(VarSource {
            name,
            owner: owner.to_path_buf(),
            scope: Scope::Private,
            kind: SourceKind::RustSchema,
            value_type: Some(value_type_from_rust(&ty).to_string()),
            enum_values: None,
            required: Some(!is_optional && default_value.is_none() && !attrs.contains("default")),
            default_value,
            description,
            value: None,
            path: path.to_path_buf(),
            line: index + 1,
        });

        pending_attrs.clear();
        pending_comments.clear();
    }

    Ok(out)
}

fn normalize_rust_description(pending_comments: &mut Vec<String>) -> Option<String> {
    let parts = std::mem::take(pending_comments)
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let description = parts.join(" ");
    (!description.is_empty()).then_some(description)
}

fn value_type_from_rust(ty: &str) -> &'static str {
    let inner = ty
        .trim()
        .strip_prefix("Option<")
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or_else(|| ty.trim())
        .trim();

    match inner {
        "bool" => "boolean",
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" | "f32" | "f64" => "number",
        _ => "string",
    }
}

pub fn upsert_schema(app: &Workspace, mutation: &VarMutation) -> Result<()> {
    let path = config_path(app);
    if !path.exists() {
        return Err(anyhow!("rust src/config.rs not found"));
    }

    remove_schema(app, &mutation.variable)?;
    let mut contents = fs::read_to_string(&path)?;
    let field_name = mutation.variable.to_lowercase();
    let rust_type = mutation_rust_type(mutation);

    let mut lines = Vec::new();
    if let Some(description) = mutation.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            lines.extend(
                description
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(|line| format!("    /// {line}")),
            );
        }
    }
    lines.push(format!("    #[serde(rename = {:?})]", mutation.variable));
    lines.push(format!("    pub {field_name}: {rust_type},"));
    lines.push(String::new());
    let field = lines.join("\n");

    if let Some(index) = contents.rfind("}\n") {
        contents.insert_str(index, &field);
    } else {
        contents.push_str(&field);
    }
    fs::write(path, contents)?;
    Ok(())
}

pub fn remove_schema(app: &Workspace, variable: &str) -> Result<()> {
    let path = config_path(app);
    if !path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(&path)?;
    let original_lines = contents.lines().collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut index = 0;

    while index < original_lines.len() {
        if original_lines[index].contains(&format!("rename = \"{variable}\"")) {
            while lines.last().is_some_and(|line: &&str| {
                let trimmed = line.trim_start();
                trimmed.starts_with("///") || trimmed.starts_with("#[")
            }) {
                lines.pop();
            }

            index += 1;
            while index < original_lines.len() {
                let trimmed = original_lines[index].trim_start();
                index += 1;
                if trimmed.starts_with("pub ") {
                    break;
                }
            }
            if index < original_lines.len() && original_lines[index].trim().is_empty() {
                index += 1;
            }
            continue;
        }

        lines.push(original_lines[index]);
        index += 1;
    }

    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

fn mutation_rust_type(mutation: &VarMutation) -> &'static str {
    let base = if mutation.boolean {
        "bool"
    } else if mutation.number || mutation.numeric {
        "f64"
    } else {
        "String"
    };

    if mutation.optional {
        match base {
            "bool" => "Option<bool>",
            "f64" => "Option<f64>",
            _ => "Option<String>",
        }
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sources_reads_serde_renames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.rs");
        fs::write(
            &path,
            r#"use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    /// Main database URL.
    #[serde(rename = "DATABASE_URL")]
    pub database_url: String,

    #[serde(rename = "SMTP_LOGIN")]
    pub smtp_login: Option<String>,

    #[serde(default = "default_log_level", rename = "LOG_LEVEL")]
    pub log_level: String,

    #[serde(rename = "PORT")]
    pub port: u16,
}
"#,
        )
        .unwrap();

        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert_eq!(sources.len(), 4);
        assert_eq!(sources[0].name, "DATABASE_URL");
        assert_eq!(sources[0].required, Some(true));
        assert_eq!(
            sources[0].description.as_deref(),
            Some("Main database URL.")
        );
        assert_eq!(sources[1].name, "SMTP_LOGIN");
        assert_eq!(sources[1].required, Some(false));
        assert_eq!(sources[3].name, "PORT");
        assert_eq!(sources[3].value_type.as_deref(), Some("number"));
    }
}
