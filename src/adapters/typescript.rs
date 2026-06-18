use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{Issue, Scope, Severity, SourceKind, VarMutation, VarSource, Workspace};
use crate::util::{is_valid_var_name, line_number_at};

pub fn private_schema_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/env.private.ts")
}

pub fn public_schema_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join("src/env.public.ts")
}

pub fn collect_private_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = private_schema_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    schema_sources(&path, &workspace.rel, Scope::Private)
}

pub fn collect_public_schema(workspace: &Workspace) -> Result<Vec<VarSource>> {
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
        if !is_valid_var_name(name) {
            continue;
        }
        let expr = expr.trim();
        let line = line_number_at(&contents, block.start + entry.find(name).unwrap_or(0));
        out.push(VarSource {
            name: name.to_string(),
            owner: owner.to_path_buf(),
            scope: scope.clone(),
            kind: SourceKind::TsSchema,
            value_type: Some(infer_type(expr)),
            required: Some(!expr.contains(".optional()") && !expr.contains(".default(")),
            default_value: extract_default(expr),
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
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => bail!("unknown scope"),
    };
    ensure_schema_file(&path, scope)?;

    remove_schema(app, &mutation.variable, scope)?;

    let mut contents = fs::read_to_string(&path)?;
    let label = if *scope == Scope::Public {
        "client"
    } else {
        "server"
    };
    let expr = expr_for_mutation(mutation);
    insert_object_entry(
        &mut contents,
        label,
        &format!("    {}: {},", mutation.variable, expr),
    )?;

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
        Scope::Public => public_schema_path(app),
        Scope::Private => private_schema_path(app),
        Scope::Unknown => bail!("unknown scope"),
    };
    ensure_schema_file(&path, scope)?;
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
    let mut new_body = Vec::new();
    for line in block.body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("{variable}:")) {
            continue;
        }
        new_body.push(line.to_string());
    }
    contents.replace_range(block.start..block.end, &new_body.join("\n"));
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

fn matching_brace(contents: &str, open: usize) -> Option<usize> {
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
    let mut escaped = false;

    for (idx, ch) in body.char_indices() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sources_ignores_jsdoc_comments_before_entries() {
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
}
