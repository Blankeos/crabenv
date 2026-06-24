use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use super::{
    gha_expression, gha_source_for, parse_sink_scope, required_value, selected_schema_records,
    source_overrides_from_captures, GhaValueSource, SinkAdapter, SinkRenderContext, SinkScope,
    GHA_ECHO_FORMAT,
};

pub(super) struct GhaEchoAdapter;

impl SinkAdapter for GhaEchoAdapter {
    fn format(&self) -> &'static str {
        GHA_ECHO_FORMAT
    }

    fn allowed_options(&self) -> &'static [&'static str] {
        &["format", "scope", "owner", "dest"]
    }

    fn parse_scope(&self, value: &str, path: &Path, line_number: usize) -> Result<SinkScope> {
        parse_sink_scope(value, path, line_number, self.format())
    }

    fn normalize_options(
        &self,
        values: &mut BTreeMap<String, String>,
        path: &Path,
        line_number: usize,
    ) -> Result<()> {
        let dest = normalize_dest(required_value(values, "dest", path, line_number)?)
            .with_context(|| {
                format!(
                    "{}:{} invalid crabenv sink dest",
                    path.display(),
                    line_number
                )
            })?;
        values.insert("dest".to_string(), dest);
        Ok(())
    }

    fn render(&self, context: SinkRenderContext<'_>) -> Result<Vec<String>> {
        let dest = context
            .directive
            .options
            .get("dest")
            .expect("gha-echo dest is validated during directive parsing");
        let source_overrides = source_overrides_from_gha_echo_lines(context.existing_body);
        let records = selected_schema_records(
            context.graph,
            &context.directive.owner,
            context.directive.scope,
        );

        Ok(records
            .into_iter()
            .map(|record| {
                let source = gha_source_for(&record.name, &record.scope, &source_overrides);
                format!(
                    "{}echo \"{}={}\" >> {}",
                    context.indent,
                    record.name,
                    gha_expression(source, &record.name),
                    dest
                )
            })
            .collect())
    }
}

fn normalize_dest(value: String) -> Result<String> {
    if value.trim().is_empty() {
        bail!("dest cannot be empty");
    }
    let path = Path::new(&value);
    if path.is_absolute() {
        bail!("dest must be relative to the project root");
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => bail!("dest cannot contain .."),
            Component::RootDir | Component::Prefix(_) => {
                bail!("dest must be relative to the project root")
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        bail!("dest cannot be .");
    }

    let dest = normalized.to_string_lossy().replace('\\', "/");
    if !dest
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        bail!("dest can only contain letters, numbers, '.', '_', '-', and '/'");
    }
    Ok(dest)
}

fn source_overrides_from_gha_echo_lines(lines: &[String]) -> BTreeMap<String, GhaValueSource> {
    let line_re = Regex::new(
        r#"^\s*echo\s+["']([A-Z_][A-Z0-9_]*)=\$\{\{\s*(vars|secrets)\.([A-Z_][A-Z0-9_]*)\s*\}\}["']\s*>>\s*(?:"[^"]+"|'[^']+'|\S+)\s*(?:#.*)?$"#,
    )
    .expect("valid gha-echo regex");
    source_overrides_from_captures(lines, &line_re)
}
