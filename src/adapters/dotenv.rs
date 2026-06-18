use anyhow::Result;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{DotenvEntry, Project, Scope, SourceKind, VarSource, Workspace};
use crate::ordering::{compare_env_names, env_group};
use crate::util::is_valid_var_name;

pub fn example_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join(".env.example")
}

#[derive(Clone, Debug)]
struct RawSection {
    header: Option<String>,
    lines: Vec<String>,
    original_index: usize,
}

#[derive(Clone, Debug)]
struct DotenvEntryBlock {
    key: String,
    lines: Vec<String>,
    original_index: usize,
}

#[derive(Clone, Debug)]
struct ParsedSection {
    header: Option<String>,
    leading: Vec<String>,
    loose: Vec<Vec<String>>,
    entries: Vec<DotenvEntryBlock>,
}

fn parse_sections(contents: &str) -> Vec<RawSection> {
    let mut sections = vec![RawSection {
        header: None,
        lines: Vec::new(),
        original_index: 0,
    }];

    for line in contents.lines() {
        if is_section_header(line) {
            sections.push(RawSection {
                header: Some(line.to_string()),
                lines: Vec::new(),
                original_index: sections.len(),
            });
        } else if let Some(section) = sections.last_mut() {
            section.lines.push(line.to_string());
        }
    }

    sections
}

fn render_section(section: RawSection) -> Vec<String> {
    let parsed = parse_section_body(section);
    let mut output = Vec::new();

    if let Some(header) = parsed.header {
        output.push(header);
    }

    push_lines(&mut output, parsed.leading);

    for loose in parsed.loose {
        if !output.is_empty() && !output.last().is_some_and(|line| line.is_empty()) {
            output.push(String::new());
        }
        push_lines(&mut output, loose);
    }

    let mut entries = parsed.entries;
    entries.sort_by(|left, right| {
        compare_env_names(&left.key, &right.key)
            .then_with(|| left.original_index.cmp(&right.original_index))
    });

    let mut previous_group: Option<String> = None;
    for entry in entries {
        let group = env_group(&entry.key);
        if previous_group
            .as_ref()
            .is_some_and(|previous| previous != &group)
            && !output.is_empty()
            && !output.last().is_some_and(|line| line.is_empty())
        {
            output.push(String::new());
        }
        push_lines(&mut output, entry.lines);
        previous_group = Some(group);
    }

    trim_blank_edges(output)
}

fn parse_section_body(section: RawSection) -> ParsedSection {
    let mut leading = Vec::new();
    let mut loose = Vec::new();
    let mut entries = Vec::new();
    let mut pending = Vec::<String>::new();
    let mut saw_entry_or_loose = false;

    for line in section.lines {
        if let Some(key) = parse_assignment_key(&line) {
            let mut lines = Vec::new();
            if saw_entry_or_loose {
                lines.append(&mut pending);
            } else if !pending.is_empty() {
                leading.append(&mut pending);
            }
            lines.push(line);
            entries.push(DotenvEntryBlock {
                key,
                lines,
                original_index: entries.len(),
            });
            saw_entry_or_loose = true;
            continue;
        }

        if line.trim().is_empty() {
            if !saw_entry_or_loose {
                if !pending.is_empty() {
                    leading.append(&mut pending);
                }
                if !leading.is_empty() && !leading.last().is_some_and(|line| line.is_empty()) {
                    leading.push(String::new());
                }
            } else if !pending.is_empty() {
                loose.push(std::mem::take(&mut pending));
            }
            continue;
        }

        if line.trim_start().starts_with('#') {
            pending.push(line);
            continue;
        }

        if !pending.is_empty() {
            loose.push(std::mem::take(&mut pending));
        }
        loose.push(vec![line]);
        saw_entry_or_loose = true;
    }

    if !pending.is_empty() {
        if saw_entry_or_loose {
            loose.push(pending);
        } else {
            leading.extend(pending);
        }
    }

    ParsedSection {
        header: section.header,
        leading,
        loose,
        entries,
    }
}

fn parse_assignment_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("export ") {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    is_valid_var_name(key).then(|| key.to_string())
}

fn is_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return false;
    }
    let body = trimmed.trim_start_matches('#').trim();
    body.starts_with("---") && body.ends_with("---")
}

fn compare_sections(left: &RawSection, right: &RawSection) -> std::cmp::Ordering {
    let left_key = section_sort_key(left);
    let right_key = section_sort_key(right);
    left_key.cmp(&right_key)
}

fn section_sort_key(section: &RawSection) -> (u8, String, usize) {
    let label = section
        .header
        .as_deref()
        .map(section_label)
        .unwrap_or_default();
    let lower = label.to_ascii_lowercase();
    let rank = if label.is_empty() && section.lines.iter().all(|line| line.trim().is_empty()) {
        4
    } else if section.header.is_none() {
        0
    } else if lower == "shared" {
        1
    } else if lower.starts_with("apps/") {
        2
    } else if section.header.is_some() {
        3
    } else {
        4
    };
    (rank, lower, section.original_index)
}

fn section_label(line: &str) -> String {
    line.trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('-')
        .trim()
        .to_string()
}

fn push_lines(output: &mut Vec<String>, lines: Vec<String>) {
    for line in lines {
        if line.is_empty() && output.last().is_some_and(|last| last.is_empty()) {
            continue;
        }
        output.push(line);
    }
}

fn trim_blank_edges(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_contents_preserves_comments_and_placeholder_values() {
        let input = r#"# Defaults/docs live in schemas
RESEND_FROM="Team <team@example.com>"
S3_SECRET_ACCESS_KEY="secret"
NODE_ENV="development"
# Bucket comment
S3_BUCKET_NAME="bucket"
RESEND_API_KEY=""
SESSION_SECRET="$(openssl rand -base64 32)"
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# Defaults/docs live in schemas
NODE_ENV="development"

RESEND_API_KEY=""
RESEND_FROM="Team <team@example.com>"

# Bucket comment
S3_BUCKET_NAME="bucket"
S3_SECRET_ACCESS_KEY="secret"

SESSION_SECRET="$(openssl rand -base64 32)"
"#
        );
    }

    #[test]
    fn format_contents_keeps_root_example_sections() {
        let input = r#"# ---- apps/web/.env.example ----
S3_BUCKET=web
NODE_ENV=development
# ---- shared ----
DATABASE_URL=file:./db
# ---- apps/api/.env.example ----
RESEND_API_KEY=
CI=true
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# ---- shared ----
DATABASE_URL=file:./db

# ---- apps/api/.env.example ----
CI=true

RESEND_API_KEY=

# ---- apps/web/.env.example ----
NODE_ENV=development

S3_BUCKET=web
"#
        );
    }
}

pub fn format_contents(contents: &str) -> String {
    if contents.trim().is_empty() {
        return String::new();
    }

    let mut sections = parse_sections(contents);
    if sections.iter().any(|section| section.header.is_some()) {
        sections.sort_by(|left, right| compare_sections(left, right));
    }

    let mut output = Vec::new();
    for section in sections {
        let rendered = render_section(section);
        if rendered.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(String::new());
        }
        output.extend(rendered);
    }

    format!("{}\n", output.join("\n").trim_end())
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
