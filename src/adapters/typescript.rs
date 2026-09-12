use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Argument, Declaration, Expression, ObjectPropertyKind, Statement};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

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
            enum_values: extract_enum_values(expr),
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

/// Validate a metadata-only TypeScript update without writing.
pub fn validate_metadata_update(
    app: &Workspace,
    variable: &str,
    scope: &Scope,
    description: Option<String>,
    optional: Option<bool>,
    default_value: Option<String>,
) -> Result<()> {
    build_metadata_contents(app, variable, scope, description, optional, default_value).map(|_| ())
}

/// Conservative metadata-only update: reuse the original zod expression
/// verbatim and only touch the outer `.optional()` / `.default(...)`
/// chain when explicitly requested. Descriptions only touch leading comments.
pub fn update_schema_metadata(
    app: &Workspace,
    variable: &str,
    scope: &Scope,
    description: Option<String>,
    optional: Option<bool>,
    default_value: Option<String>,
) -> Result<()> {
    let (path, next) =
        build_metadata_contents(app, variable, scope, description, optional, default_value)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, next)?;
    Ok(())
}

fn build_metadata_contents(
    app: &Workspace,
    variable: &str,
    scope: &Scope,
    description: Option<String>,
    optional: Option<bool>,
    default_value: Option<String>,
) -> Result<(PathBuf, String)> {
    let (path, label) = match scope {
        Scope::Public if should_use_plain_schema(app) => (plain_schema_path(app), "client"),
        Scope::Private if should_use_plain_schema(app) => (plain_schema_path(app), "server"),
        Scope::Public => (public_schema_path(app), "client"),
        Scope::Private => (private_schema_path(app), "server"),
        Scope::Unknown => bail!("unknown scope for '{variable}'"),
    };
    if !path.exists() {
        bail!("TypeScript schema not found: {}", path.display());
    }
    let contents = fs::read_to_string(&path)?;
    let analysis = analyze_schema_expr(&contents, label, variable).ok_or_else(|| {
        anyhow!(
            "'{variable}' was not found in {label} object; edit the file manually or use an explicit type flag to recreate it"
        )
    })?;

    let new_expr = if let Some(default) = default_value.as_deref() {
        if !analysis.is_z {
            bail!(unsupported_msg(variable, &analysis.raw));
        }
        let lit = default_literal(&analysis.kind, default, variable)?;
        format!("{}{}", analysis.base.trim(), lit)
    } else if let Some(want_optional) = optional {
        if !analysis.is_z {
            bail!(unsupported_msg(variable, &analysis.raw));
        }
        if want_optional {
            format!("{}.optional()", analysis.base.trim())
        } else {
            analysis.base.trim().to_string()
        }
    } else {
        analysis.raw.trim().to_string()
    };

    let new_leading_opt: Option<String> = match description {
        None => None,
        Some(desc) => {
            let trimmed = desc.trim();
            if trimmed.is_empty() {
                Some(String::new())
            } else {
                Some(
                    trimmed
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(|line| format!("    // {line}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        }
    };

    let mut next = contents.clone();
    if new_expr != analysis.raw.trim() {
        let (s, e) = analysis.value_span;
        next.replace_range(s..e, &new_expr);
    }
    if let Some(new_leading) = new_leading_opt {
        let (ls, le) = leading_comment_range(&next, analysis.prop_start);
        if new_leading.is_empty() {
            if ls != le {
                next.replace_range(ls..le, "");
            }
        } else if ls == le {
            next.insert_str(ls, &format!("{new_leading}\n"));
        } else {
            next.replace_range(ls..le, &format!("{new_leading}\n"));
        }
    }
    Ok((path, next))
}

fn unsupported_msg(variable: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    format!(
        "crabenv update {variable}: unsupported zod expression '{trimmed}'; use explicit --string/--numeric/--number/--boolean/--enum/--testRegex to replace it or edit the file manually"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZodKind {
    Number,
    Boolean,
    StringLike,
    Unknown,
}

struct SchemaAnalysis {
    raw: String,
    base: String,
    kind: ZodKind,
    is_z: bool,
    prop_start: usize,
    value_span: (usize, usize),
}

fn analyze_schema_expr(contents: &str, label: &str, variable: &str) -> Option<SchemaAnalysis> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, contents, SourceType::ts()).parse();
    let program = ret.program;
    let (prop_span, value_expr) = find_schema_property(&program, label, variable)?;
    let value_span = value_expr.span();
    let raw = contents
        .get(value_span.start as usize..value_span.end as usize)?
        .to_string();
    let base_expr = strip_root_optional_default(value_expr).ok()?;
    let base_span = base_expr.span();
    let base = contents
        .get(base_span.start as usize..base_span.end as usize)?
        .to_string();
    let (is_z, kind) = zod_kind(base_expr);
    Some(SchemaAnalysis {
        raw,
        base,
        kind,
        is_z,
        prop_start: prop_span.start as usize,
        value_span: (value_span.start as usize, value_span.end as usize),
    })
}

fn find_schema_property<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    label: &str,
    variable: &str,
) -> Option<(oxc_span::Span, &'a Expression<'a>)> {
    for stmt in &program.body {
        let decls = match stmt {
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(var_decl)) => Some(&var_decl.declarations),
                _ => None,
            },
            Statement::VariableDeclaration(var_decl) => Some(&var_decl.declarations),
            _ => None,
        };
        let Some(decls) = decls else { continue };
        for decl in decls {
            let Some(init) = &decl.init else { continue };
            if let Some(found) = find_in_create_env(init, label, variable) {
                return Some(found);
            }
        }
    }
    None
}

fn find_in_create_env<'a>(
    init: &'a Expression<'a>,
    label: &str,
    variable: &str,
) -> Option<(oxc_span::Span, &'a Expression<'a>)> {
    let mut cur = init;
    loop {
        match cur {
            Expression::ParenthesizedExpression(paren) => cur = &paren.expression,
            Expression::TSAsExpression(e) => cur = &e.expression,
            Expression::TSSatisfiesExpression(e) => cur = &e.expression,
            Expression::TSNonNullExpression(e) => cur = &e.expression,
            _ => break,
        }
    }
    let Expression::CallExpression(call) = cur else {
        return None;
    };
    for arg in &call.arguments {
        let Argument::ObjectExpression(obj) = arg else {
            continue;
        };
        for prop_kind in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop_kind else {
                continue;
            };
            if prop.key.static_name().as_deref() != Some(label) {
                continue;
            }
            let Expression::ObjectExpression(inner) = &prop.value else {
                continue;
            };
            for inner_kind in &inner.properties {
                let ObjectPropertyKind::ObjectProperty(inner_prop) = inner_kind else {
                    continue;
                };
                if inner_prop.key.static_name().as_deref() == Some(variable) {
                    return Some((inner_prop.span, &inner_prop.value));
                }
            }
        }
    }
    None
}

/// Surgically strip outer `.optional()` / `.default(...)` calls using AST.
fn strip_root_optional_default<'a>(mut expr: &'a Expression<'a>) -> Result<&'a Expression<'a>> {
    loop {
        match expr {
            Expression::CallExpression(call) => {
                let Expression::StaticMemberExpression(member) = &call.callee else {
                    break;
                };
                let name = member.property.name.as_str();
                if name == "optional" {
                    if !call.arguments.is_empty() {
                        bail!("unsupported optional with args");
                    }
                    expr = &member.object;
                } else if name == "default" {
                    if call.arguments.len() != 1 {
                        bail!("unsupported default arity");
                    }
                    expr = &member.object;
                } else {
                    break;
                }
            }
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::ChainExpression(_)
            | Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSTypeAssertion(_) => {
                bail!("unsupported wrapper")
            }
            _ => break,
        }
    }
    Ok(expr)
}

fn zod_kind(expr: &Expression) -> (bool, ZodKind) {
    let mut names = Vec::new();
    let mut is_z = false;
    let mut unsupported = false;
    collect_chain(expr, &mut names, &mut is_z, &mut unsupported);
    if unsupported || !is_z {
        return (is_z, ZodKind::Unknown);
    }
    if names
        .iter()
        .any(|n| n == "preprocess" || n == "transform" || n == "pipe")
    {
        return (true, ZodKind::Unknown);
    }
    if names.iter().any(|n| n == "number") {
        return (true, ZodKind::Number);
    }
    if names.iter().any(|n| n == "boolean") {
        return (true, ZodKind::Boolean);
    }
    if names.iter().any(|n| n == "string" || n == "enum") {
        return (true, ZodKind::StringLike);
    }
    (true, ZodKind::Unknown)
}

fn collect_chain(
    expr: &Expression,
    names: &mut Vec<String>,
    is_z: &mut bool,
    unsupported: &mut bool,
) {
    match expr {
        Expression::CallExpression(call) => {
            collect_chain(&call.callee, names, is_z, unsupported);
        }
        Expression::StaticMemberExpression(member) => {
            collect_chain(&member.object, names, is_z, unsupported);
            names.push(member.property.name.as_str().to_string());
        }
        Expression::ComputedMemberExpression(member) => {
            collect_chain(&member.object, names, is_z, unsupported);
            *unsupported = true;
        }
        Expression::PrivateFieldExpression(_) => *unsupported = true,
        Expression::Identifier(ident) => {
            if ident.name.as_str() == "z" {
                *is_z = true;
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_chain(&paren.expression, names, is_z, unsupported);
        }
        Expression::ChainExpression(_)
        | Expression::TSAsExpression(_)
        | Expression::TSSatisfiesExpression(_)
        | Expression::TSNonNullExpression(_)
        | Expression::TSTypeAssertion(_) => *unsupported = true,
        _ => {}
    }
}

fn default_literal(kind: &ZodKind, default: &str, variable: &str) -> Result<String> {
    match kind {
        ZodKind::Number => {
            if !is_valid_number_literal(default) {
                bail!(
                    "crabenv update {variable}: invalid default '{default}' for number schema; edit the file manually"
                );
            }
            Ok(format!(".default({})", default.trim()))
        }
        ZodKind::Boolean => {
            let trimmed = default.trim();
            if trimmed != "true" && trimmed != "false" {
                bail!(
                    "crabenv update {variable}: invalid default '{default}' for boolean schema; edit the file manually"
                );
            }
            Ok(format!(".default({trimmed})"))
        }
        ZodKind::StringLike => Ok(format!(".default({:?})", default)),
        ZodKind::Unknown => bail!(
            "crabenv update {variable}: unsupported zod expression for default; use explicit --string/--numeric/--number/--boolean/--enum/--testRegex to replace it or edit the file manually"
        ),
    }
}

fn is_valid_number_literal(value: &str) -> bool {
    let s = value.trim();
    let s = s
        .strip_prefix('+')
        .or_else(|| s.strip_prefix('-'))
        .unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    let (mantissa, exp) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    if let Some(e) = exp {
        let e = e
            .strip_prefix('+')
            .or_else(|| e.strip_prefix('-'))
            .unwrap_or(e);
        if e.is_empty() || !e.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    match mantissa.split_once('.') {
        Some((int, frac)) => {
            if int.is_empty() || frac.is_empty() {
                return false;
            }
            int.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
        }
        None => !mantissa.is_empty() && mantissa.bytes().all(|b| b.is_ascii_digit()),
    }
}

fn leading_comment_range(contents: &str, prop_start: usize) -> (usize, usize) {
    let prop_start = prop_start.min(contents.len());
    let line_start = contents[..prop_start]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut cur = line_start;
    let mut leading_start = line_start;
    loop {
        if cur == 0 {
            break;
        }
        let prev_end = cur - 1;
        let prev_start = contents[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let mut line = &contents[prev_start..prev_end];
        line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if !is_comment_only_line(trimmed) {
            break;
        }
        leading_start = prev_start;
        cur = prev_start;
    }
    (leading_start, line_start)
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
    let body = &contents[block.start..block.end];
    let insert_at = block.start + trailing_comment_block_start(body).unwrap_or(body.len());
    let body_before_insert = &contents[block.start..insert_at];
    let insertion = if body_before_insert.trim().is_empty() {
        format!("\n{line}\n  ")
    } else if body_before_insert.ends_with('\n') {
        format!("{line}\n")
    } else {
        format!("\n{line}\n  ")
    };
    contents.insert_str(insert_at, &insertion);
    Ok(())
}

fn trailing_comment_block_start(body: &str) -> Option<usize> {
    let lines = body_lines(body);
    let mut index = lines.len();

    while index > 0 && lines[index - 1].2.trim().is_empty() {
        index -= 1;
    }

    let mut found_comment = false;
    let mut start = None;
    while index > 0 {
        let (line_start, _, line) = lines[index - 1];
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_only_line(trimmed) {
            if is_comment_only_line(trimmed) {
                found_comment = true;
            }
            start = Some(line_start);
            index -= 1;
        } else {
            break;
        }
    }

    found_comment.then_some(start.unwrap_or(body.len()))
}

fn body_lines(body: &str) -> Vec<(usize, usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, ch) in body.char_indices() {
        if ch == '\n' {
            lines.push((start, index + ch.len_utf8(), &body[start..index]));
            start = index + ch.len_utf8();
        }
    }
    if start < body.len() {
        lines.push((start, body.len(), &body[start..]));
    }
    lines
}

fn is_comment_only_line(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("*/")
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
    let mut last_sortable_index = None;
    for (index, entry) in entries.into_iter().enumerate() {
        if let Some(key) = entry_key(&entry) {
            last_sortable_index = Some(index);
            sortable.push(SchemaEntry { key, entry, index });
        } else {
            loose.push(LooseSchemaEntry { entry, index });
        }
    }

    sortable.sort_by(|left, right| {
        crate::ordering::compare_env_names(&left.key, &right.key)
            .then_with(|| left.index.cmp(&right.index))
    });

    let mut body_lines = Vec::new();
    let trailing_start = last_sortable_index
        .map(|index| index + 1)
        .unwrap_or(usize::MAX);
    for entry in loose.iter().filter(|entry| entry.index < trailing_start) {
        push_schema_entry(&mut body_lines, &entry.entry, false);
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

    for entry in loose.iter().filter(|entry| entry.index >= trailing_start) {
        if !body_lines.is_empty() && !body_lines.last().is_some_and(|line| line.is_empty()) {
            body_lines.push(String::new());
        }
        push_schema_entry(&mut body_lines, &entry.entry, false);
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

#[derive(Debug)]
struct LooseSchemaEntry {
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
    fn format_schema_contents_keeps_trailing_comment_blocks_at_end() {
        let contents = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    RESEND_API_KEY: z.string(),
    NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
    DATABASE_URL: z.string(),
    // SMTP
    // SMTP_HOST: z.string(),
    // SMTP_PORT: z.preprocess(Number, z.number()),
    // SMTP_SECURE: z.preprocess((val) => String(val).toLowerCase() === 'true', z.boolean()),
    // SMTP_USER: z.string(),
    // SMTP_PASS: z.string(),
    // SMTP_FROM: z.string(),
  },
});
"#;

        let formatted = format_schema_contents(contents, &Scope::Private).unwrap();

        assert!(formatted.contains(
            r#"    NODE_ENV: z.enum(["development", "test", "production"]).default("development"),

    DATABASE_URL: z.string(),

    RESEND_API_KEY: z.string(),

    // SMTP
    // SMTP_HOST: z.string(),
    // SMTP_PORT: z.preprocess(Number, z.number()),
    // SMTP_SECURE: z.preprocess((val) => String(val).toLowerCase() === 'true', z.boolean()),
    // SMTP_USER: z.string(),
    // SMTP_PASS: z.string(),
    // SMTP_FROM: z.string(),"#
        ));
    }

    #[test]
    fn insert_object_entry_adds_before_trailing_comment_blocks() {
        let mut contents = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    PORT: z.coerce.number().default(3000),

    // SMTP - in case you want something other than zeptomail
    // SMTP_HOST: z.string(),
    // SMTP_PORT: z.preprocess(Number, z.number()),
    // SMTP_SECURE: z.preprocess((val) => String(val).toLowerCase() === 'true', z.boolean()),
    // SMTP_USER: z.string(),
    // SMTP_PASS: z.string(),
    // SMTP_FROM: z.string(),
  },
});
"#
        .to_string();

        insert_object_entry(&mut contents, "server", "    DATABASE_URL: z.string(),").unwrap();

        assert!(contents.contains(
            r#"    PORT: z.coerce.number().default(3000),
    DATABASE_URL: z.string(),

    // SMTP - in case you want something other than zeptomail"#
        ));
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

    fn metadata_test_workspace(dir: &tempfile::TempDir) -> Workspace {
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string().url(),
    API_KEY: z.string().min(32),
    TOKEN: z.string().regex(/^[A-Z]+$/),
  },
});
"#,
        )
        .unwrap();
        Workspace {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        }
    }

    #[test]
    fn metadata_description_preserves_url_min_and_regex() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = metadata_test_workspace(&dir);

        update_schema_metadata(
            &workspace,
            "DATABASE_URL",
            &Scope::Private,
            Some("DB docs".to_string()),
            None,
            None,
        )
        .unwrap();
        update_schema_metadata(
            &workspace,
            "API_KEY",
            &Scope::Private,
            Some("Key docs".to_string()),
            None,
            None,
        )
        .unwrap();
        update_schema_metadata(
            &workspace,
            "TOKEN",
            &Scope::Private,
            Some("Token docs".to_string()),
            None,
            None,
        )
        .unwrap();

        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("DATABASE_URL: z.string().url(),"));
        assert!(contents.contains("API_KEY: z.string().min(32),"));
        assert!(contents.contains(r"TOKEN: z.string().regex(/^[A-Z]+$/),"));
        assert!(!contents.contains(r"/^-?\d+(\.\d+)?$/"));
    }

    #[test]
    fn metadata_optional_and_default_keep_validators() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = metadata_test_workspace(&dir);

        update_schema_metadata(
            &workspace,
            "API_KEY",
            &Scope::Private,
            None,
            Some(true),
            None,
        )
        .unwrap();
        update_schema_metadata(
            &workspace,
            "DATABASE_URL",
            &Scope::Private,
            None,
            None,
            Some("https://example.com".to_string()),
        )
        .unwrap();

        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("API_KEY: z.string().min(32).optional(),"));
        assert!(
            contents.contains(r#"DATABASE_URL: z.string().url().default("https://example.com"),"#)
        );
    }

    #[test]
    fn metadata_unsupported_syntax_fails_before_writes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

const helper = z.string();
export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    WEIRD: helper,
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
        let before = fs::read_to_string(private_schema_path(&workspace)).unwrap();

        let err =
            update_schema_metadata(&workspace, "WEIRD", &Scope::Private, None, Some(true), None)
                .unwrap_err();

        assert!(err.to_string().contains("unsupported"));
        assert_eq!(
            fs::read_to_string(private_schema_path(&workspace)).unwrap(),
            before
        );
    }

    #[test]
    fn metadata_description_on_helper_schema_works() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

const helper = z.string();
export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    WEIRD: helper,
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
        update_schema_metadata(
            &workspace,
            "WEIRD",
            &Scope::Private,
            Some("Helper docs".to_string()),
            None,
            None,
        )
        .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("// Helper docs"));
        assert!(contents.contains("WEIRD: helper,"));
    }

    #[test]
    fn metadata_regex_with_slashes_quotes_and_delimiters() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    COMPLEX: z.string().regex(/^a\/b(,c)?["']+$/, "needs / and \"quotes\""),
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
        update_schema_metadata(
            &workspace,
            "COMPLEX",
            &Scope::Private,
            Some("Complex docs".to_string()),
            None,
            None,
        )
        .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains(
            r#"COMPLEX: z.string().regex(/^a\/b(,c)?["']+$/, "needs / and \"quotes\""),"#
        ));

        update_schema_metadata(
            &workspace,
            "COMPLEX",
            &Scope::Private,
            None,
            Some(true),
            None,
        )
        .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains(
            r#"COMPLEX: z.string().regex(/^a\/b(,c)?["']+$/, "needs / and \"quotes\"").optional(),"#
        ));
    }

    #[test]
    fn metadata_embedded_comments_preserved() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    TOKEN: z.string() /* embedded: keep */ .regex(/^[A-Z]+$/),
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
        update_schema_metadata(
            &workspace,
            "TOKEN",
            &Scope::Private,
            Some("Token docs".to_string()),
            None,
            None,
        )
        .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("/* embedded: keep */"));
        assert!(contents.contains(r"TOKEN: z.string() /* embedded: keep */ .regex(/^[A-Z]+$/),"));

        update_schema_metadata(&workspace, "TOKEN", &Scope::Private, None, Some(true), None)
            .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("/* embedded: keep */"));
        assert!(contents.contains(r".regex(/^[A-Z]+$/).optional(),"));
    }

    #[test]
    fn metadata_invalid_number_default_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    PORT: z.coerce.number(),
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
        let before = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        for bad in ["not-a-number", "process.exit()", "\"123\""] {
            let err = update_schema_metadata(
                &workspace,
                "PORT",
                &Scope::Private,
                None,
                None,
                Some(bad.to_string()),
            )
            .unwrap_err();
            assert!(err.to_string().contains("invalid") || err.to_string().contains("unsupported"));
            assert_eq!(
                fs::read_to_string(private_schema_path(&workspace)).unwrap(),
                before
            );
        }
        update_schema_metadata(
            &workspace,
            "PORT",
            &Scope::Private,
            None,
            None,
            Some("3000".to_string()),
        )
        .unwrap();
        let contents = fs::read_to_string(private_schema_path(&workspace)).unwrap();
        assert!(contents.contains("PORT: z.coerce.number().default(3000),"));
    }
}
