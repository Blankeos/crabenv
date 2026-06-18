use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{Issue, Scope, Severity, SourceKind, VarMutation, VarSource, Workspace};
use crate::util::{is_valid_var_name, line_number_at};

pub fn private_schema_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/env.private.ts")
}

fn ensure_plain_schema_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("schema path has no parent"))?;
    fs::create_dir_all(parent)?;
    fs::write(
        path,
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const env = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  server: {},
  client: {},
  runtimeEnvStrict: {},
});
"#,
    )?;
    Ok(())
}

pub fn active_schema_paths(workspace: &Workspace) -> Vec<(PathBuf, Scope)> {
    if should_use_plain_schema(workspace) {
        let path = plain_schema_path(workspace);
        return vec![(path.clone(), Scope::Private), (path, Scope::Public)];
    }

    vec![
        (private_schema_path(workspace), Scope::Private),
        (public_schema_path(workspace), Scope::Public),
    ]
}

pub fn public_schema_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/env.public.ts")
}

pub fn plain_schema_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/env.ts")
}

pub fn should_use_plain_schema(workspace: &Workspace) -> bool {
    plain_schema_path(workspace).exists()
        && !private_schema_path(workspace).exists()
        && !public_schema_path(workspace).exists()
}

pub fn collect_private_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
    if should_use_plain_schema(workspace) {
        return schema_sources(
            &plain_schema_path(workspace),
            &workspace.rel,
            Scope::Private,
        );
    }

    let path = private_schema_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    schema_sources(&path, &workspace.rel, Scope::Private)
}

pub fn collect_public_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
    if should_use_plain_schema(workspace) {
        return schema_sources(&plain_schema_path(workspace), &workspace.rel, Scope::Public);
    }

    let path = public_schema_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    schema_sources(&path, &workspace.rel, Scope::Public)
}

pub fn schema_sources(path: &Path, owner: &Path, scope: Scope) -> Result<Vec<VarSource>> {
    let contents = fs::read_to_string(path)?;
    let label = match scope {
        Scope::Private => "server",
        Scope::Public => "client",
        Scope::Unknown => return Ok(Vec::new()),
    };
    let Some(block) = extract_object_block(&contents, label) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for entry in split_object_entries(&block.body) {
        let cleaned_entry = strip_comments(&entry);
        let Some((name, expr)) = cleaned_entry.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_matches('"').trim_matches('\'');
        if !is_valid_var_name(name) {
            continue;
        }
        let expr = expr.trim();
        let line = line_number_at(&contents, block.start + entry.find(name).unwrap_or(0));
        let description = description_from_schema_entry(&entry);
        out.push(VarSource {
            name: name.to_string(),
            owner: owner.to_path_buf(),
            scope: scope.clone(),
            kind: SourceKind::TsSchema,
            value_type: Some(infer_type(expr)),
            required: Some(!expr.contains(".optional()") && !expr.contains(".default(")),
            default_value: extract_default(expr),
            description,
            value: None,
            path: path.to_path_buf(),
            line,
        });
    }
    Ok(out)
}

pub fn check_public_runtime_strict(
    path: &Path,
    owner: &Path,
    issues: &mut Vec<Issue>,
) -> Result<()> {
    let contents = fs::read_to_string(path)?;
    if !contents.contains("runtimeEnvStrict") {
        issues.push(Issue {
            severity: Severity::Error,
            message: format!(
                "{} public schema must use runtimeEnvStrict",
                crate::util::display_rel(owner)
            ),
            fix: None,
        });
        return Ok(());
    }

    let client_vars = schema_sources(path, owner, Scope::Public)?
        .into_iter()
        .map(|source| source.name)
        .collect::<Vec<_>>();
    let strict_block = extract_object_block(&contents, "runtimeEnvStrict")
        .map(|block| block.body)
        .unwrap_or_default();
    for name in client_vars {
        if !strict_block.contains(&name) {
            issues.push(Issue {
                severity: Severity::Error,
                message: format!(
                    "{} is public in {} but missing from runtimeEnvStrict",
                    name,
                    crate::util::display_rel(owner)
                ),
                fix: None,
            });
        }
    }
    Ok(())
}

pub fn format_schema_contents(contents: &str, scope: &Scope) -> Result<String> {
    let mut contents = contents.to_string();
    match scope {
        Scope::Private => format_object_entries(&mut contents, "server")?,
        Scope::Public => {
            format_object_entries(&mut contents, "client")?;
            format_object_entries(&mut contents, "runtimeEnvStrict")?;
        }
        Scope::Unknown => {}
    }
    Ok(contents)
}

pub fn upsert_schema(app: &Workspace, mutation: &VarMutation, scope: &Scope) -> Result<()> {
    let path = match scope {
        Scope::Public if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Private if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => bail!("unknown scope"),
    };
    if should_use_plain_schema(app) {
        ensure_plain_schema_file(&path)?;
    } else {
        ensure_schema_file(&path, scope)?;
    }

    remove_schema(app, &mutation.variable, scope)?;

    let mut contents = fs::read_to_string(&path)?;
    let label = if *scope == Scope::Public {
        "client"
    } else {
        "server"
    };
    let expr = expr_for_mutation(mutation);
    let entry = schema_entry_for_mutation(mutation, &expr);
    insert_object_entry(&mut contents, label, &entry)?;

    if *scope == Scope::Public {
        insert_object_entry(
            &mut contents,
            "runtimeEnvStrict",
            &format!(
                "    {}: process.env.{},",
                mutation.variable, mutation.variable
            ),
        )?;
    }

    fs::write(path, contents)?;
    Ok(())
}

pub fn remove_schema(app: &Workspace, variable: &str, scope: &Scope) -> Result<()> {
    let path = match scope {
        Scope::Public if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Private if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => return Ok(()),
    };
    if !path.exists() {
        return Ok(());
    }
    let mut contents = fs::read_to_string(&path)?;
    let label = if *scope == Scope::Public {
        "client"
    } else {
        "server"
    };
    remove_object_entry(&mut contents, label, variable)?;
    if *scope == Scope::Public {
        remove_object_entry(&mut contents, "runtimeEnvStrict", variable)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

pub fn read_schema_expr(app: &Workspace, variable: &str, scope: &Scope) -> Result<Option<String>> {
    let path = match scope {
        Scope::Public if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Private if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path)?;
    let label = if *scope == Scope::Public {
        "client"
    } else {
        "server"
    };
    Ok(find_schema_expr(&contents, label, variable))
}

pub fn upsert_schema_expr(
    app: &Workspace,
    variable: &str,
    scope: &Scope,
    expr: &str,
) -> Result<()> {
    let path = match scope {
        Scope::Public if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Private if should_use_plain_schema(app) => plain_schema_path(app),
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => bail!("unknown scope"),
    };
    if should_use_plain_schema(app) {
        ensure_plain_schema_file(&path)?;
    } else {
        ensure_schema_file(&path, scope)?;
    }
    remove_schema(app, variable, scope)?;

    let mut contents = fs::read_to_string(&path)?;
    let label = if *scope == Scope::Public {
        "client"
    } else {
        "server"
    };
    insert_object_entry(
        &mut contents,
        label,
        &format!("    {}: {},", variable, expr.trim()),
    )?;

    if *scope == Scope::Public {
        insert_object_entry(
            &mut contents,
            "runtimeEnvStrict",
            &format!("    {}: process.env.{},", variable, variable),
        )?;
    }

    fs::write(path, contents)?;
    Ok(())
}

fn ensure_schema_file(path: &Path, scope: &Scope) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("schema path has no parent"))?;
    fs::create_dir_all(parent)?;
    let contents = match scope {
        Scope::Private => {
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {},
});
"#
        }
        Scope::Public => {
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  client: {},
  runtimeEnvStrict: {},
});
"#
        }
        Scope::Unknown => bail!("unknown scope"),
    };
    fs::write(path, contents)?;
    Ok(())
}

pub fn extract_object_block(contents: &str, label: &str) -> Option<ObjectBlock> {
    let needle = format!("{label}:");
    let label_start = contents.find(&needle)?;
    let after_label = label_start + needle.len();
    let open_rel = contents[after_label..].find('{')?;
    let open = after_label + open_rel;
    let close = matching_brace(contents, open)?;
    Some(ObjectBlock {
        body: contents[open + 1..close].to_string(),
        start: open + 1,
        end: close,
    })
}

#[derive(Debug)]
pub struct ObjectBlock {
    pub body: String,
    pub start: usize,
    pub end: usize,
}

fn insert_object_entry(contents: &mut String, label: &str, line: &str) -> Result<()> {
    let block = extract_object_block(contents, label)
        .ok_or_else(|| anyhow!("could not find {label} object in TypeScript schema"))?;
    let insert_at = block.end;
    let body = &contents[block.start..block.end];
    let insertion = if body.trim().is_empty() {
        format!("\n{line}\n  ")
    } else if body.ends_with('\n') {
        format!("{line}\n")
    } else {
        format!("\n{line}\n  ")
    };
    contents.insert_str(insert_at, &insertion);
    Ok(())
}

fn find_schema_expr(contents: &str, label: &str, variable: &str) -> Option<String> {
    let block = extract_object_block(contents, label)?;
    let body_without_comments = strip_comments(&block.body);
    for entry in split_object_entries(&body_without_comments) {
        let cleaned_entry = entry
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let Some((name, expr)) = cleaned_entry.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_matches('"').trim_matches('\'');
        if name == variable {
            return Some(expr.trim().trim_end_matches(',').trim().to_string());
        }
    }
    None
}

fn remove_object_entry(contents: &mut String, label: &str, variable: &str) -> Result<()> {
    let Some(block) = extract_object_block(contents, label) else {
        return Ok(());
    };
    let entries = split_object_entries(&block.body);
    if entries.is_empty() {
        return Ok(());
    }

    let mut removed = false;
    let mut body_lines = Vec::new();
    for entry in entries {
        if entry_key(&entry).as_deref() == Some(variable) {
            removed = true;
            continue;
        }
        let ensure_comma = entry_key(&entry).is_some();
        push_schema_entry(&mut body_lines, &entry, ensure_comma);
    }

    if !removed {
        return Ok(());
    }

    let new_body = if body_lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n  ", body_lines.join("\n").trim_end())
    };
    contents.replace_range(block.start..block.end, &new_body);
    Ok(())
}

fn format_object_entries(contents: &mut String, label: &str) -> Result<()> {
    let Some(block) = extract_object_block(contents, label) else {
        return Ok(());
    };
    let entries = split_object_entries(&block.body);
    if entries.is_empty() {
        return Ok(());
    }

    let mut sortable = Vec::new();
    let mut loose = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        if let Some(key) = entry_key(&entry) {
            sortable.push(SchemaEntry { key, entry, index });
        } else {
            loose.push(entry);
        }
    }

    sortable.sort_by(|left, right| {
        crate::ordering::compare_env_names(&left.key, &right.key)
            .then_with(|| left.index.cmp(&right.index))
    });

    let mut body_lines = Vec::new();
    for entry in loose {
        push_schema_entry(&mut body_lines, &entry, false);
    }

    let mut previous_group: Option<String> = None;
    for entry in sortable {
        let group = crate::ordering::env_group(&entry.key);
        if previous_group
            .as_ref()
            .is_some_and(|previous| previous != &group)
            && !body_lines.is_empty()
            && !body_lines.last().is_some_and(|line| line.is_empty())
        {
            body_lines.push(String::new());
        }
        push_schema_entry(&mut body_lines, &entry.entry, true);
        previous_group = Some(group);
    }

    let new_body = if body_lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n  ", body_lines.join("\n").trim_end())
    };
    contents.replace_range(block.start..block.end, &new_body);
    Ok(())
}

#[derive(Debug)]
struct SchemaEntry {
    key: String,
    entry: String,
    index: usize,
}

fn entry_key(entry: &str) -> Option<String> {
    let without_comments = strip_comments(entry);
    let (name, _) = without_comments.split_once(':')?;
    let name = name.trim().trim_matches('"').trim_matches('\'');
    is_valid_var_name(name).then(|| name.to_string())
}

fn push_schema_entry(output: &mut Vec<String>, entry: &str, ensure_comma: bool) {
    let start = output.len();
    for line in normalize_schema_entry(entry) {
        output.push(format!("    {line}"));
    }

    if ensure_comma {
        if let Some(last) = output[start..]
            .iter_mut()
            .rev()
            .find(|line| !line.trim().is_empty())
        {
            if !last.trim_end().ends_with(',') {
                last.push(',');
            }
        }
    }
}

fn normalize_schema_entry(entry: &str) -> Vec<String> {
    entry
        .trim()
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line.trim_start().to_string()
            } else {
                line.strip_prefix("    ").unwrap_or(line).to_string()
            }
        })
        .collect()
}

fn strip_comments(contents: &str) -> String {
    let mut output = String::new();
    let mut chars = contents.chars().peekable();
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = Some(ch);
                output.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        output.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if next == '\n' {
                        output.push('\n');
                    }
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => output.push(ch),
        }
    }

    output
}

fn description_from_schema_entry(entry: &str) -> Option<String> {
    let key_line_index = entry.lines().position(|line| entry_key(line).is_some())?;
    let leading = entry.lines().take(key_line_index).collect::<Vec<_>>();
    description_from_comment_lines(&leading)
}

fn description_from_comment_lines(lines: &[&str]) -> Option<String> {
    let mut text = Vec::new();
    let mut in_block = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !text.is_empty() {
                break;
            }
            continue;
        }

        if in_block {
            if let Some(end) = trimmed.find("*/") {
                let before_end = &trimmed[..end];
                push_comment_text(&mut text, before_end);
                in_block = false;
                if !trimmed[end + 2..].trim().is_empty() {
                    break;
                }
                continue;
            }
            push_comment_text(&mut text, trimmed);
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("//") {
            text.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("/**") {
            if let Some(end) = rest.find("*/") {
                push_comment_text(&mut text, &rest[..end]);
            } else {
                push_comment_text(&mut text, rest);
                in_block = true;
            }
        } else if let Some(rest) = trimmed.strip_prefix("/*") {
            if let Some(end) = rest.find("*/") {
                push_comment_text(&mut text, &rest[..end]);
            } else {
                push_comment_text(&mut text, rest);
                in_block = true;
            }
        } else {
            break;
        }
    }

    normalize_description_parts(text)
}

fn push_comment_text(parts: &mut Vec<String>, raw: &str) {
    let cleaned = raw.trim().trim_start_matches('*').trim();
    if !cleaned.is_empty() {
        parts.push(cleaned.to_string());
    }
}

fn normalize_description_parts(parts: Vec<String>) -> Option<String> {
    let description = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!description.is_empty()).then_some(description)
}

fn matching_brace(contents: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut escaped = false;

    let mut chars = contents[open..].char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }

        if in_block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '*') => {
                chars.next();
                in_block_comment = true;
            }
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_object_entries(body: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut curly = 0usize;
    let mut square = 0usize;
    let mut paren = 0usize;
    let mut in_string: Option<char> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut escaped = false;

    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }

        if in_block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '*') => {
                chars.next();
                in_block_comment = true;
            }
            '{' => curly += 1,
            '}' => curly = curly.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            ',' if curly == 0 && square == 0 && paren == 0 => {
                let entry = body[start..idx].trim();
                if !entry.is_empty() {
                    entries.push(entry.to_string());
                }
                start = idx + 1;
            }
            _ => {}
        }
    }

    let entry = body[start..].trim();
    if !entry.is_empty() {
        entries.push(entry.to_string());
    }
    entries
}

fn infer_type(expr: &str) -> String {
    if expr.contains("z.enum") {
        extract_enum_values(expr)
            .map(|values| format!("enum({})", values.len()))
            .unwrap_or_else(|| "enum".to_string())
    } else if expr.contains("z.coerce.number") {
        "number".to_string()
    } else if expr.contains("z.coerce.boolean") {
        "boolean".to_string()
    } else if expr.contains(".url()") {
        "url".to_string()
    } else if expr.contains(".regex(") {
        "regex".to_string()
    } else {
        "string".to_string()
    }
}

fn extract_enum_values(expr: &str) -> Option<Vec<String>> {
    let start = expr.find("z.enum([")? + "z.enum([".len();
    let end = expr[start..].find(']')? + start;
    let values = expr[start..end]
        .split(',')
        .map(|value| value.trim().trim_matches('"').trim_matches('\''))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn extract_default(expr: &str) -> Option<String> {
    let index = expr.find(".default(")?;
    let start = index + ".default(".len();
    let end = matching_paren(expr, start - 1)?;
    Some(
        expr[start..end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string(),
    )
}

fn matching_paren(contents: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (offset, ch) in contents[open..].char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn expr_for_mutation(mutation: &VarMutation) -> String {
    let mut expr = if let Some(values) = &mutation.enum_values {
        let values = values
            .split(',')
            .map(|value| format!("{:?}", value.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("z.enum([{values}])")
    } else if mutation.number {
        "z.coerce.number()".to_string()
    } else if mutation.boolean {
        "z.coerce.boolean()".to_string()
    } else if mutation.numeric {
        r#"z.string().regex(/^-?\d+(\.\d+)?$/)"#.to_string()
    } else if let Some(regex) = &mutation.test_regex {
        let message = mutation
            .test_regex_message
            .as_ref()
            .map(|value| format!(", {:?}", value))
            .unwrap_or_default();
        format!("z.string().regex(/{regex}/{message})")
    } else {
        "z.string()".to_string()
    };

    if let Some(default_value) = &mutation.default_value {
        if mutation.number || mutation.boolean {
            expr.push_str(&format!(".default({default_value})"));
        } else {
            expr.push_str(&format!(".default({default_value:?})"));
        }
    } else if mutation.optional {
        expr.push_str(".optional()");
    }

    expr
}

fn schema_entry_for_mutation(mutation: &VarMutation, expr: &str) -> String {
    let entry = format!("    {}: {},", mutation.variable, expr);
    let Some(description) = mutation.description.as_deref().map(str::trim) else {
        return entry;
    };
    if description.is_empty() {
        return entry;
    }

    let comment = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("    // {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    if comment.is_empty() {
        entry
    } else {
        format!("{comment}\n{entry}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sources_uses_comments_before_entries_as_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env.private.ts");
        fs::write(
            &path,
            r#"
import { createEnv } from "@t3-oss/env-core";
import z from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    /** Database URL. */
    DATABASE_URL: z.string(),
    /** Optional in development. */
    DATABASE_AUTH_TOKEN: z
      .string()
      .optional()
      .refine((val) => (process.env.NODE_ENV !== "development" ? !!val : true)),
    // Regular line comments should still be ignored.
    ZEPTOMAIL_FROM: z.string().refine((val) => /^[^<]*\s<[^>]+>$/.test(val), {
      message: 'Must be in "Name <email@example.com>" format',
    }),
  },
});
"#,
        )
        .unwrap();

        let sources = schema_sources(&path, Path::new("."), Scope::Private).unwrap();
        let names = sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec!["DATABASE_URL", "DATABASE_AUTH_TOKEN", "ZEPTOMAIL_FROM"]
        );
        assert_eq!(sources[0].required, Some(true));
        assert_eq!(sources[1].required, Some(false));
        assert_eq!(sources[0].description.as_deref(), Some("Database URL."));
        assert_eq!(
            sources[1].description.as_deref(),
            Some("Optional in development.")
        );
        assert_eq!(
            sources[2].description.as_deref(),
            Some("Regular line comments should still be ignored.")
        );
    }

    #[test]
    fn schema_sources_combines_multiline_comment_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env.private.ts");
        fs::write(
            &path,
            r#"
import { createEnv } from "@t3-oss/env-core";
import z from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    /**
     * API key used by the billing worker.
     * Stored in the vendor dashboard.
     */
    BILLING_API_KEY: z.string(),
  },
});
"#,
        )
        .unwrap();

        let sources = schema_sources(&path, Path::new("."), Scope::Private).unwrap();

        assert_eq!(
            sources[0].description.as_deref(),
            Some("API key used by the billing worker. Stored in the vendor dashboard.")
        );
    }

    #[test]
    fn format_schema_contents_sorts_entries_and_keeps_comments() {
        let contents = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    RESEND_FROM: z.string(),
    /** S3 bucket. */
    S3_BUCKET_NAME: z.string(),
    NODE_ENV: z.enum(["development", "production"]).default("development"),
    RESEND_API_KEY: z.string(),
  },
});
"#;

        let formatted = format_schema_contents(contents, &Scope::Private).unwrap();

        assert!(formatted.contains(
            r#"  server: {
    NODE_ENV: z.enum(["development", "production"]).default("development"),

    RESEND_API_KEY: z.string(),
    RESEND_FROM: z.string(),

    /** S3 bucket. */
    S3_BUCKET_NAME: z.string(),
  },"#
        ));
    }

    #[test]
    fn format_schema_contents_does_not_split_commas_inside_line_comments() {
        let contents = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    // Local sqlite URL in development, managed database URL elsewhere.
    DATABASE_URL: z.string(),
    NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  },
});
"#;

        let formatted = format_schema_contents(contents, &Scope::Private).unwrap();

        assert!(formatted.contains(
            r#"    // Local sqlite URL in development, managed database URL elsewhere.
    DATABASE_URL: z.string(),"#
        ));
        assert!(!formatted.contains("\n    managed database URL elsewhere."));
    }

    #[test]
    fn upsert_schema_writes_description_comment() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };

        upsert_schema(
            &workspace,
            &VarMutation {
                variable: "DATABASE_URL".to_string(),
                description: Some("Local sqlite URL in development".to_string()),
                example: None,
                optional: false,
                default_value: None,
                numeric: false,
                number: false,
                boolean: false,
                enum_values: None,
                test_regex: None,
                test_regex_message: None,
            },
            &Scope::Private,
        )
        .unwrap();

        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains(
            r#"    // Local sqlite URL in development
    DATABASE_URL: z.string(),"#
        ));
    }

    #[test]
    fn upsert_schema_replaces_old_description_comment() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };

        upsert_schema(
            &workspace,
            &VarMutation {
                variable: "DATABASE_URL".to_string(),
                description: Some("Old description".to_string()),
                example: None,
                optional: false,
                default_value: None,
                numeric: false,
                number: false,
                boolean: false,
                enum_values: None,
                test_regex: None,
                test_regex_message: None,
            },
            &Scope::Private,
        )
        .unwrap();
        upsert_schema(
            &workspace,
            &VarMutation {
                variable: "DATABASE_URL".to_string(),
                description: Some("New description".to_string()),
                example: None,
                optional: false,
                default_value: None,
                numeric: false,
                number: false,
                boolean: false,
                enum_values: None,
                test_regex: None,
                test_regex_message: None,
            },
            &Scope::Private,
        )
        .unwrap();

        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains(
            r#"    // New description
    DATABASE_URL: z.string(),"#
        ));
        assert!(!contents.contains("Old description"));
    }

    #[test]
    fn plain_env_ts_collects_server_as_private_and_client_as_public() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("env.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const env = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  server: {
    DATABASE_URL: z.string().url(),
  },
  client: {
    PUBLIC_APP_URL: z.string().url(),
  },
  runtimeEnvStrict: {
    PUBLIC_APP_URL: process.env.PUBLIC_APP_URL,
  },
});
"#,
        )
        .unwrap();
        let workspace = Workspace {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };

        let private = collect_private_schema(&workspace).unwrap();
        let public = collect_public_schema(&workspace).unwrap();

        assert!(should_use_plain_schema(&workspace));
        assert_eq!(private.len(), 1);
        assert_eq!(private[0].name, "DATABASE_URL");
        assert_eq!(private[0].scope, Scope::Private);
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].name, "PUBLIC_APP_URL");
        assert_eq!(public[0].scope, Scope::Public);
    }

    #[test]
    fn split_schema_files_take_precedence_over_plain_env_ts() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("env.ts"),
            r#"export const env = createEnv({ server: { PLAIN_ONLY: z.string() }, client: {} });"#,
        )
        .unwrap();
        fs::write(
            src.join("env.private.ts"),
            r#"export const privateEnv = createEnv({ server: { SPLIT_ONLY: z.string() } });"#,
        )
        .unwrap();
        let workspace = Workspace {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };

        let private = collect_private_schema(&workspace).unwrap();

        assert!(!should_use_plain_schema(&workspace));
        assert_eq!(private.len(), 1);
        assert_eq!(private[0].name, "SPLIT_ONLY");
    }
}
