use anyhow::Result;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

use super::{
    gha_expression, gha_source_for, parse_sink_scope, selected_schema_records,
    source_overrides_from_captures, GhaValueSource, SinkAdapter, SinkRenderContext, SinkScope,
    GHA_ENV_FORMAT,
};

pub(super) struct GhaEnvAdapter;

impl SinkAdapter for GhaEnvAdapter {
    fn format(&self) -> &'static str {
        GHA_ENV_FORMAT
    }

    fn allowed_options(&self) -> &'static [&'static str] {
        &["format", "scope", "owner"]
    }

    fn parse_scope(&self, value: &str, path: &Path, line_number: usize) -> Result<SinkScope> {
        parse_sink_scope(value, path, line_number, self.format())
    }

    fn render(&self, context: SinkRenderContext<'_>) -> Result<Vec<String>> {
        let source_overrides = source_overrides_from_gha_env_lines(context.existing_body);
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
                    "{}{}: {}",
                    context.indent,
                    record.name,
                    gha_expression(source, &record.name)
                )
            })
            .collect())
    }
}

fn source_overrides_from_gha_env_lines(lines: &[String]) -> BTreeMap<String, GhaValueSource> {
    let line_re = Regex::new(
        r#"^\s*([A-Z_][A-Z0-9_]*)\s*:\s*["']?\$\{\{\s*(vars|secrets)\.([A-Z_][A-Z0-9_]*)\s*\}\}["']?\s*(?:#.*)?$"#,
    )
    .expect("valid gha-env regex");
    source_overrides_from_captures(lines, &line_re)
}
