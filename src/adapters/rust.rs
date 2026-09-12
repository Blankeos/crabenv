use crate::models::{Scope, SourceKind, VarMutation, VarSource, Workspace};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
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
    let file = parse_rust(&contents)?;
    let Some(target) = target_struct(&file)? else {
        return Ok(Vec::new());
    };
    let syn::Fields::Named(named) = &target.fields else {
        return Err(anyhow!("unsupported struct: expected named fields"));
    };
    let mut out = Vec::new();
    for field in &named.named {
        let (rename, has_default, default_fn) = serde_info(field);
        let Some(rename) = rename else { continue };
        if !is_env_name(&rename) {
            continue;
        }
        out.push(VarSource {
            name: rename,
            owner: owner.to_path_buf(),
            scope: Scope::Private,
            kind: SourceKind::RustSchema,
            value_type: Some(value_type_from_ty(&field.ty).to_string()),
            enum_values: None,
            required: Some(option_inner(&field.ty).is_none() && !has_default),
            default_value: default_fn,
            description: field_description(field),
            value: None,
            path: path.to_path_buf(),
            line: field
                .ident
                .as_ref()
                .map(|i| i.span().start().line)
                .unwrap_or(1),
        });
    }
    Ok(out)
}
pub fn upsert_schema(app: &Workspace, mutation: &VarMutation) -> Result<()> {
    let path = config_path(app);
    if !path.exists() {
        return Err(anyhow!("rust src/config.rs not found"));
    }
    let original = fs::read_to_string(&path)?;
    fs::write(path, build_upsert_contents(&original, mutation)?)?;
    Ok(())
}
pub fn remove_schema(app: &Workspace, variable: &str) -> Result<()> {
    let path = config_path(app);
    if !path.exists() {
        return Ok(());
    }
    let contents = fs::read_to_string(&path)?;
    let next = build_remove_contents(&contents, variable)?;
    if next != contents {
        fs::write(path, next)?;
    }
    Ok(())
}
fn build_upsert_contents(original: &str, mutation: &VarMutation) -> Result<String> {
    let file = parse_rust(original)?;
    let Some(target) = target_struct(&file)? else {
        return Err(anyhow!(
            "rust src/config.rs missing Settings or Config struct"
        ));
    };
    let syn::Fields::Named(named) = &target.fields else {
        return Err(anyhow!("unsupported struct: expected named fields"));
    };
    let mut after_remove = original.to_string();
    apply_removals(
        &mut after_remove,
        removal_ranges(original, named, &mutation.variable)?,
    )?;
    let file = parse_rust(&after_remove)?;
    let Some(target) = target_struct(&file)? else {
        return Err(anyhow!(
            "rust src/config.rs missing Settings or Config struct"
        ));
    };
    let syn::Fields::Named(named) = &target.fields else {
        return Err(anyhow!("unsupported struct: expected named fields"));
    };
    let (open_end, close_start) = brace_ranges(&after_remove, target)?;
    let field = render_field(mutation);
    let mut next = after_remove.clone();
    if after_remove[open_end..close_start].trim().is_empty() {
        check_range(&after_remove, open_end, close_start)?;
        next.replace_range(open_end..close_start, &format!("\n{field}"));
    } else {
        let missing_comma = if named.named.is_empty() {
            None
        } else {
            match named.named.pairs().next_back() {
                Some(syn::punctuated::Pair::End(last)) => {
                    Some(byte_range(last.span(), &after_remove)?.1)
                }
                _ => None,
            }
        };
        let insertion = if after_remove[..close_start].ends_with('\n') {
            field
        } else {
            format!("\n{field}")
        };
        check_range(&after_remove, close_start, close_start)?;
        next.insert_str(close_start, &insertion);
        if let Some(pos) = missing_comma {
            check_range(&next, pos, pos)?;
            next.insert(pos, ',');
        }
    }
    parse_rust(&next).map_err(|err| anyhow!("generated invalid rust src/config.rs: {err}"))?;
    Ok(next)
}
fn build_remove_contents(contents: &str, variable: &str) -> Result<String> {
    let file = parse_rust(contents)?;
    let Some(target) = target_struct(&file)? else {
        return Ok(contents.to_string());
    };
    let syn::Fields::Named(named) = &target.fields else {
        return Err(anyhow!("unsupported struct: expected named fields"));
    };
    let removals = removal_ranges(contents, named, variable)?;
    if removals.is_empty() {
        return Ok(contents.to_string());
    }
    let mut next = contents.to_string();
    apply_removals(&mut next, removals)?;
    parse_rust(&next).map_err(|err| anyhow!("generated invalid rust src/config.rs: {err}"))?;
    Ok(next)
}
fn apply_removals(contents: &mut String, mut removals: Vec<(usize, usize)>) -> Result<()> {
    removals.sort_by_key(|range| std::cmp::Reverse(range.0));
    for (start, end) in removals {
        check_range(contents, start, end)?;
        contents.replace_range(start..end, "");
    }
    Ok(())
}
fn parse_rust(contents: &str) -> Result<syn::File> {
    syn::parse_file(contents).map_err(|err| anyhow!("failed to parse rust src/config.rs: {err}"))
}
// Top-level Settings (preferred) or Config named struct. Parser ignores
// comments/strings/nested mods, so fakes there never match. Duplicates are
// ambiguous; unit/tuple structs are unsupported and must not fall through.
fn target_struct(file: &syn::File) -> Result<Option<&syn::ItemStruct>> {
    for name in ["Settings", "Config"] {
        let found: Vec<&syn::ItemStruct> = file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Struct(s) if s.ident == name => Some(s),
                _ => None,
            })
            .collect();
        if found.is_empty() {
            continue;
        }
        if found.len() > 1 {
            return Err(anyhow!("ambiguous {name} struct"));
        }
        if !matches!(found[0].fields, syn::Fields::Named(_)) {
            return Err(anyhow!("unsupported {name} struct: expected named fields"));
        }
        return Ok(Some(found[0]));
    }
    Ok(None)
}
// Single pass over #[serde(..)]; consumes `= value` for every key so
// `default = ".."` before `rename = ".."` cannot poison the parse.
fn serde_info(field: &syn::Field) -> (Option<String>, bool, Option<String>) {
    let mut rename = None;
    let mut has_default = false;
    let mut default_fn = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let v: syn::LitStr = meta.value()?.parse()?;
                if rename.is_none() {
                    rename = Some(v.value());
                }
            } else if meta.path.is_ident("default") {
                has_default = true;
                if meta.input.peek(syn::Token![=]) {
                    if let Ok(v) = meta.value()?.parse::<syn::LitStr>() {
                        if default_fn.is_none() {
                            default_fn = Some(v.value());
                        }
                    }
                }
            } else if meta.input.peek(syn::Token![=]) {
                let _: Option<syn::Expr> = meta.value()?.parse().ok();
            }
            Ok(())
        });
    }
    (rename, has_default, default_fn)
}
fn field_description(field: &syn::Field) -> Option<String> {
    let parts: Vec<String> = field
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            syn::Meta::NameValue(p) => match &p.value {
                syn::Expr::Lit(l) => match &l.lit {
                    syn::Lit::Str(s) => {
                        let t = s.value().trim().to_string();
                        (!t.is_empty()).then_some(t)
                    }
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}
fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(a) = &seg.arguments else {
        return None;
    };
    if a.args.len() != 1 {
        return None;
    }
    match &a.args[0] {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}
fn value_type_from_ty(ty: &syn::Type) -> &'static str {
    let inner = option_inner(ty).unwrap_or(ty);
    if let syn::Type::Path(p) = inner {
        if let Some(seg) = p.path.segments.last() {
            if seg.ident == "bool" {
                return "boolean";
            }
            if matches!(
                seg.ident.to_string().as_str(),
                "u8" | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "f32"
                    | "f64"
            ) {
                return "number";
            }
            return "string";
        }
    }
    "string"
}
fn is_env_name(name: &str) -> bool {
    let mut c = name.chars();
    c.next().is_some_and(|f| f.is_ascii_uppercase())
        && c.all(|x| x.is_ascii_uppercase() || x.is_ascii_digit() || x == '_')
}
fn byte_range(span: proc_macro2::Span, contents: &str) -> Result<(usize, usize)> {
    let r = span.byte_range();
    check_range(contents, r.start, r.end)?;
    Ok((r.start, r.end))
}
fn check_range(contents: &str, start: usize, end: usize) -> Result<()> {
    if start > end || end > contents.len() {
        return Err(anyhow!("invalid rust span {start}..{end}"));
    }
    if !contents.is_char_boundary(start) || !contents.is_char_boundary(end) {
        return Err(anyhow!("rust span splits unicode character"));
    }
    Ok(())
}
// Ranges for matching fields plus trailing comma; start absorbs indentation,
// end absorbs one newline to avoid whitespace-only lines. Unicode-safe via spans.
fn removal_ranges(
    contents: &str,
    named: &syn::FieldsNamed,
    variable: &str,
) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    for pair in named.named.pairs() {
        let (field, comma) = match pair {
            syn::punctuated::Pair::Punctuated(f, c) => (f, Some(c)),
            syn::punctuated::Pair::End(f) => (f, None),
        };
        if serde_info(field).0.as_deref() != Some(variable) {
            continue;
        }
        let (mut start, mut end) = byte_range(field.span(), contents)?;
        if let Some(c) = comma {
            end = end.max(byte_range(c.span(), contents)?.1);
        }
        while start > 0 && matches!(contents.as_bytes()[start - 1], b' ' | b'\t') {
            start -= 1;
        }
        if contents.as_bytes().get(end) == Some(&b'\n') {
            end += 1;
        } else if contents.as_bytes().get(end) == Some(&b'\r')
            && contents.as_bytes().get(end + 1) == Some(&b'\n')
        {
            end += 2;
        }
        check_range(contents, start, end)?;
        out.push((start, end));
    }
    Ok(out)
}
fn brace_ranges(contents: &str, item: &syn::ItemStruct) -> Result<(usize, usize)> {
    let syn::Fields::Named(named) = &item.fields else {
        return Err(anyhow!("unsupported struct: expected named fields"));
    };
    let open = named.brace_token.span.open().byte_range();
    let close = named.brace_token.span.close().byte_range();
    check_range(contents, open.start, open.end)?;
    check_range(contents, close.start, close.end)?;
    Ok((open.end, close.start))
}
fn render_field(mutation: &VarMutation) -> String {
    let mut lines = Vec::new();
    if let Some(d) = mutation.description.as_deref().map(str::trim) {
        if !d.is_empty() {
            lines.extend(
                d.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(|l| format!("    /// {l}")),
            );
        }
    }
    lines.push(format!("    #[serde(rename = {:?})]", mutation.variable));
    lines.push(format!(
        "    pub {}: {},",
        mutation.variable.to_lowercase(),
        mutation_rust_type(mutation)
    ));
    lines.push(String::new());
    lines.join("\n")
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

    fn test_workspace(root: &Path) -> Workspace {
        Workspace {
            root: root.to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "rust".to_string(),
        }
    }

    fn test_mutation(variable: &str) -> VarMutation {
        VarMutation {
            variable: variable.to_string(),
            description: Some("Test description".to_string()),
            example: None,
            optional: false,
            default_value: None,
            numeric: false,
            number: false,
            boolean: false,
            enum_values: None,
            test_regex: None,
            test_regex_message: None,
        }
    }

    fn docs_style_config() -> String {
        r#"use figment::{providers::{Env, Format, Toml}, Figment};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "DATABASE_URL")]
    pub database_url: String,

    #[serde(rename = "SECRET_KEY")]
    pub secret_key: String,

    #[serde(default = "default_log_level", rename = "LOG_LEVEL")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Settings {
    pub fn load() -> Result<Self, figment::Error> {
        dotenvy::dotenv().ok();

        Figment::new()
            .merge(Toml::file("Config.toml").nested())
            .merge(Env::raw())
            .extract()
    }
}
"#
        .to_string()
    }

    fn assert_valid_rust(contents: &str) {
        syn::parse_file(contents).expect("generated file must parse as Rust");
    }

    #[test]
    fn upsert_targets_settings_struct_when_helpers_follow() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        fs::write(&path, docs_style_config()).unwrap();
        let workspace = test_workspace(dir.path());

        upsert_schema(&workspace, &test_mutation("NEW_API_KEY")).unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        let new_pos = updated
            .find("rename = \"NEW_API_KEY\"")
            .expect("new field should be inserted");
        let struct_pos = updated
            .find("pub struct Settings")
            .expect("Settings struct should remain");
        let helper_pos = updated
            .find("fn default_log_level")
            .expect("default helper should remain");
        let impl_pos = updated
            .find("impl Settings")
            .expect("impl block should remain");

        assert!(
            struct_pos < new_pos,
            "new field should be after Settings declaration"
        );
        assert!(
            new_pos < helper_pos,
            "new field should be before trailing helper fn (regression: was appended at final brace)"
        );
        assert!(
            new_pos < impl_pos,
            "new field should be before impl block, not inside it"
        );

        // Preserve unrelated code.
        assert!(updated.contains("rename = \"DATABASE_URL\""));
        assert!(updated.contains("rename = \"SECRET_KEY\""));
        assert!(updated.contains("fn default_log_level() -> String {"));
        assert!(updated.contains("\"info\".to_string()"));
        assert!(updated.contains("Figment::new()"));

        // The new field must live inside Settings, not inside impl.
        let (before_impl, after_impl) = updated.split_at(impl_pos);
        assert!(before_impl.contains("rename = \"NEW_API_KEY\""));
        assert!(!after_impl.contains("rename = \"NEW_API_KEY\""));

        // Updated file must still expose the new variable via schema collection.
        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert!(sources.iter().any(|source| source.name == "NEW_API_KEY"));
    }

    #[test]
    fn upsert_into_inline_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = "use serde::Deserialize;\n\npub struct Config {}";
        fs::write(&path, original).unwrap();
        let workspace = test_workspace(dir.path());

        upsert_schema(&workspace, &test_mutation("INLINE_KEY")).unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        assert!(updated.contains("rename = \"INLINE_KEY\""));
        assert!(updated.contains("pub inline_key: String,"));
        let struct_pos = updated.find("pub struct Config").unwrap();
        let field_pos = updated.find("rename = \"INLINE_KEY\"").unwrap();
        assert!(struct_pos < field_pos);
        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "INLINE_KEY");
    }

    #[test]
    fn ignores_fake_struct_in_comments_and_raw_strings() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = r##"use serde::Deserialize;

/*
pub struct Settings {
    #[serde(rename = "FAKE_BLOCK")]
    pub fake_block: String,
}
*/

/// Not a struct: pub struct Settings { }
const FAKE_RAW: &str = r#"pub struct Settings {
    #[serde(rename = "FAKE_RAW")]
    pub fake_raw: String,
}"#;

// pub struct Settings { pub fake_line: String, }

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "REAL_ONE")]
    pub real_one: String,
}
"##;
        fs::write(&path, original).unwrap();

        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "REAL_ONE");

        let workspace = test_workspace(dir.path());
        upsert_schema(&workspace, &test_mutation("NEW_KEY")).unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        // Fakes are preserved verbatim.
        assert!(updated.contains("FAKE_BLOCK"));
        assert!(updated.contains("FAKE_RAW"));
        assert!(updated.contains("const FAKE_RAW"));
        // New field is inside the real struct only.
        assert_eq!(updated.matches("rename = \"NEW_KEY\"").count(), 1);
        let real_pos = updated.find("pub struct Settings").unwrap();
        // The last real struct is the actual one; fakes appear earlier.
        let new_pos = updated.find("rename = \"NEW_KEY\"").unwrap();
        assert!(new_pos > real_pos);
        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert!(sources.iter().any(|s| s.name == "NEW_KEY"));
        assert!(!sources.iter().any(|s| s.name == "FAKE_BLOCK"));
    }

    #[test]
    fn remove_retains_same_rename_in_unrelated_struct() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = r#"use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "SHARED_KEY")]
    pub shared_key: String,

    #[serde(rename = "ONLY_SETTINGS")]
    pub only_settings: String,
}

#[derive(Debug, Deserialize)]
pub struct Other {
    #[serde(rename = "SHARED_KEY")]
    pub shared_key: String,
}
"#;
        fs::write(&path, original).unwrap();
        let workspace = test_workspace(dir.path());

        remove_schema(&workspace, "SHARED_KEY").unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        // Settings lost its copy, Other kept its copy.
        assert_eq!(updated.matches("rename = \"SHARED_KEY\"").count(), 1);
        assert!(updated.contains("pub struct Other"));
        assert!(updated.contains("pub struct Settings"));
        assert!(updated.contains("rename = \"ONLY_SETTINGS\""));
        let other_pos = updated.find("pub struct Other").unwrap();
        let remaining = updated[other_pos..].contains("rename = \"SHARED_KEY\"");
        assert!(remaining, "unrelated struct must retain its field");
        let settings_body = &updated[..other_pos];
        assert!(
            !settings_body.contains("rename = \"SHARED_KEY\""),
            "target struct must lose its field"
        );
    }

    #[test]
    fn upsert_replaces_duplicates_and_ignores_plain_fields() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = r#"use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(rename = "DUP_KEY")]
    pub dup_key: String,

    pub plain_without_rename: String,

    #[serde(rename = "DUP_KEY")]
    pub dup_key_again: String,
}
"#;
        fs::write(&path, original).unwrap();
        let sources = schema_sources(&path, Path::new(".")).unwrap();
        assert_eq!(sources.iter().filter(|s| s.name == "DUP_KEY").count(), 2);
        assert!(!sources.iter().any(|s| s.name == "PLAIN_WITHOUT_RENAME"));

        let workspace = test_workspace(dir.path());
        upsert_schema(&workspace, &test_mutation("DUP_KEY")).unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        assert_eq!(updated.matches("rename = \"DUP_KEY\"").count(), 1);
        assert!(updated.contains("pub plain_without_rename: String,"));
        assert!(updated.contains("Test description"));
    }

    #[test]
    fn upsert_errors_without_mutating_when_struct_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = "use serde::Deserialize;\n\npub struct Other {\n    pub foo: String,\n}\n";
        fs::write(&path, original).unwrap();
        let workspace = test_workspace(dir.path());

        let result = upsert_schema(&workspace, &test_mutation("NEW_API_KEY"));
        assert!(result.is_err());

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, original, "file must be untouched on error");
    }

    #[test]
    fn upsert_errors_without_mutating_on_unsupported_unit_struct() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = "use serde::Deserialize;\n\npub struct Settings;\n";
        fs::write(&path, original).unwrap();
        let workspace = test_workspace(dir.path());

        let result = upsert_schema(&workspace, &test_mutation("NEW_API_KEY"));
        assert!(result.is_err());

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, original, "file must be untouched on error");
    }

    #[test]
    fn unicode_comments_stay_valid_through_upsert_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let path = dir.path().join("src/config.rs");
        let original = "use serde::Deserialize;\n\n// héllo wörld 🌍\n#[derive(Debug, Deserialize)]\npub struct Settings {\n    /// Déscription with émoji 🎉\n    #[serde(rename = \"UNICODE_KEY\")]\n    pub unicode_key: String,\n}\n";
        fs::write(&path, original).unwrap();
        let workspace = test_workspace(dir.path());

        upsert_schema(&workspace, &test_mutation("NEW_UNI")).unwrap();
        let updated = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&updated);
        assert!(updated.contains("héllo"));
        assert!(updated.contains("Déscription"));

        remove_schema(&workspace, "UNICODE_KEY").unwrap();
        let cleaned = fs::read_to_string(&path).unwrap();
        assert_valid_rust(&cleaned);
        assert!(!cleaned.contains("rename = \"UNICODE_KEY\""));
        assert!(cleaned.contains("rename = \"NEW_UNI\""));
    }
}
