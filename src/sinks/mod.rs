use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::graph::EnvGraph;
use crate::models::{EnvSurface, FileWritePlan, Project, Scope};
use crate::ordering::compare_env_names;
use crate::util::display_rel;

mod gha_echo;
mod gha_env;

const START_MARKER: &str = "crabenv:start";
const END_MARKER: &str = "crabenv:end";
pub(super) const GHA_ENV_FORMAT: &str = "gha-env";
pub(super) const GHA_ECHO_FORMAT: &str = "gha-echo";

#[derive(Clone, Debug)]
pub struct SinkPlan {
    pub changed_block_count: usize,
    pub writes: Vec<FileWritePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SinkScope {
    Public,
    Private,
    All,
}

#[derive(Clone, Debug)]
pub(super) struct SinkDirective {
    format: String,
    pub(super) scope: SinkScope,
    pub(super) owner: PathBuf,
    pub(super) options: BTreeMap<String, String>,
}

pub(super) struct SinkRenderContext<'a> {
    pub(super) graph: &'a EnvGraph,
    pub(super) directive: &'a SinkDirective,
    pub(super) indent: &'a str,
    pub(super) existing_body: &'a [String],
}

pub(super) trait SinkAdapter {
    fn format(&self) -> &'static str;
    fn allowed_options(&self) -> &'static [&'static str];
    fn parse_scope(&self, value: &str, path: &Path, line_number: usize) -> Result<SinkScope>;
    fn normalize_options(
        &self,
        _values: &mut BTreeMap<String, String>,
        _path: &Path,
        _line_number: usize,
    ) -> Result<()> {
        Ok(())
    }
    fn render(&self, context: SinkRenderContext<'_>) -> Result<Vec<String>>;
}

#[derive(Clone, Debug)]
struct RenderedFile {
    contents: String,
    changed_block_count: usize,
}

#[derive(Clone, Debug)]
pub(super) struct SinkRecord {
    pub(super) name: String,
    pub(super) scope: Scope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GhaValueSource {
    Vars,
    Secrets,
}

pub fn build_sink_plan(project: &Project, graph: &EnvGraph) -> Result<SinkPlan> {
    let mut changed_block_count = 0;
    let mut writes = Vec::new();

    for path in workflow_paths(project)? {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let rendered = render_sink_file(project, graph, &path, &contents)?;
        changed_block_count += rendered.changed_block_count;
        if rendered.contents != contents {
            writes.push(FileWritePlan {
                path,
                contents: rendered.contents,
            });
        }
    }

    Ok(SinkPlan {
        changed_block_count,
        writes,
    })
}

pub fn apply_sink_plan(project: &Project, graph: &EnvGraph) -> Result<SinkPlan> {
    let plan = build_sink_plan(project, graph)?;
    for write in &plan.writes {
        fs::write(&write.path, &write.contents)
            .with_context(|| format!("failed to write {}", write.path.display()))?;
    }
    Ok(plan)
}

fn workflow_paths(project: &Project) -> Result<Vec<PathBuf>> {
    let workflows = project.root.join(".github/workflows");
    if !workflows.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&workflows)
        .with_context(|| format!("failed to read {}", workflows.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if matches!(extension, "yml" | "yaml") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn render_sink_file(
    project: &Project,
    graph: &EnvGraph,
    path: &Path,
    contents: &str,
) -> Result<RenderedFile> {
    let line_ending = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = contents.ends_with('\n');
    let normalized = contents.replace("\r\n", "\n");
    let lines = normalized.lines().map(str::to_string).collect::<Vec<_>>();
    let mut output = Vec::<String>::new();
    let mut changed_block_count = 0;
    let mut index = 0;

    while index < lines.len() {
        let line = &lines[index];
        if !line.contains(START_MARKER) {
            if line.contains(END_MARKER) {
                bail!(
                    "{}:{} has crabenv:end without crabenv:start",
                    path.display(),
                    index + 1
                );
            }
            output.push(line.clone());
            index += 1;
            continue;
        }

        let directive = parse_directive(project, path, line, index + 1)?;
        let indent = leading_whitespace(line);
        let end_index = find_end_marker(path, &lines, index + 1)?;
        let existing_body = lines[index + 1..end_index].to_vec();
        let generated = render_block(graph, &directive, &indent, &existing_body)?;
        if existing_body != generated {
            changed_block_count += 1;
        }
        output.push(line.clone());
        output.extend(generated);
        output.push(lines[end_index].clone());
        index = end_index + 1;
    }

    let mut rendered = output.join("\n");
    if had_trailing_newline {
        rendered.push('\n');
    }
    if line_ending == "\r\n" {
        rendered = rendered.replace('\n', "\r\n");
    }

    Ok(RenderedFile {
        contents: rendered,
        changed_block_count,
    })
}

fn find_end_marker(path: &Path, lines: &[String], start_index: usize) -> Result<usize> {
    for (index, line) in lines.iter().enumerate().skip(start_index) {
        if line.contains(START_MARKER) {
            bail!(
                "{}:{} has nested crabenv:start before crabenv:end",
                path.display(),
                index + 1
            );
        }
        if line.contains(END_MARKER) {
            return Ok(index);
        }
    }

    bail!(
        "{}:{} has crabenv:start without crabenv:end",
        path.display(),
        start_index
    )
}

fn parse_directive(
    project: &Project,
    path: &Path,
    line: &str,
    line_number: usize,
) -> Result<SinkDirective> {
    let Some((_, rest)) = line.split_once(START_MARKER) else {
        bail!(
            "{}:{} invalid crabenv sink marker",
            path.display(),
            line_number
        );
    };
    let tokens = shlex::split(rest.trim()).ok_or_else(|| {
        anyhow::anyhow!(
            "{}:{} invalid crabenv sink directive quoting",
            path.display(),
            line_number
        )
    })?;
    let mut values = BTreeMap::<String, String>::new();
    for token in tokens {
        let Some((key, value)) = token.split_once('=') else {
            bail!(
                "{}:{} invalid crabenv sink token `{}`; expected key=value",
                path.display(),
                line_number,
                token
            );
        };
        if values.insert(key.to_string(), value.to_string()).is_some() {
            bail!(
                "{}:{} duplicate crabenv sink option `{}`",
                path.display(),
                line_number,
                key
            );
        }
    }

    let format = required_value(&values, "format", path, line_number)?;
    let adapter = adapter_for_format(&format).ok_or_else(|| {
        anyhow::anyhow!(
            "{}:{} unsupported crabenv sink format `{}`; supported formats: {}",
            path.display(),
            line_number,
            format,
            supported_formats()
        )
    })?;

    for key in values.keys() {
        if !adapter.allowed_options().contains(&key.as_str()) {
            bail!(
                "{}:{} unsupported crabenv sink option `{}` for format={}",
                path.display(),
                line_number,
                key,
                adapter.format()
            );
        }
    }

    adapter.normalize_options(&mut values, path, line_number)?;

    let scope = adapter.parse_scope(
        &required_value(&values, "scope", path, line_number)?,
        path,
        line_number,
    )?;

    let owner = normalize_owner(required_value(&values, "owner", path, line_number)?)
        .with_context(|| {
            format!(
                "{}:{} invalid crabenv sink owner",
                path.display(),
                line_number
            )
        })?;
    if !project
        .workspaces
        .iter()
        .any(|workspace| workspace.rel == owner)
    {
        bail!(
            "{}:{} unknown crabenv sink owner `{}`",
            path.display(),
            line_number,
            display_rel(&owner)
        );
    }

    Ok(SinkDirective {
        format: adapter.format().to_string(),
        scope,
        owner,
        options: values,
    })
}

pub(super) fn required_value(
    values: &BTreeMap<String, String>,
    key: &str,
    path: &Path,
    line_number: usize,
) -> Result<String> {
    values.get(key).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "{}:{} missing required crabenv sink option `{}`",
            path.display(),
            line_number,
            key
        )
    })
}

fn normalize_owner(value: String) -> Result<PathBuf> {
    if value.trim().is_empty() {
        bail!("owner cannot be empty");
    }
    let path = Path::new(&value);
    if path.is_absolute() {
        bail!("owner must be relative to the project root");
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => bail!("owner cannot contain .."),
            Component::RootDir | Component::Prefix(_) => {
                bail!("owner must be relative to the project root")
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(normalized)
    }
}

fn render_block(
    graph: &EnvGraph,
    directive: &SinkDirective,
    indent: &str,
    existing_body: &[String],
) -> Result<Vec<String>> {
    let adapter = adapter_for_format(&directive.format)
        .ok_or_else(|| anyhow::anyhow!("unsupported crabenv sink format `{}`", directive.format))?;
    adapter.render(SinkRenderContext {
        graph,
        directive,
        indent,
        existing_body,
    })
}

fn adapter_for_format(format: &str) -> Option<Box<dyn SinkAdapter>> {
    match format {
        GHA_ENV_FORMAT => Some(Box::new(gha_env::GhaEnvAdapter)),
        GHA_ECHO_FORMAT => Some(Box::new(gha_echo::GhaEchoAdapter)),
        _ => None,
    }
}

fn supported_formats() -> &'static str {
    "gha-env, gha-echo"
}

pub(super) fn parse_sink_scope(
    value: &str,
    path: &Path,
    line_number: usize,
    format: &str,
) -> Result<SinkScope> {
    match value {
        "public" => Ok(SinkScope::Public),
        "private" => Ok(SinkScope::Private),
        "all" => Ok(SinkScope::All),
        other => bail!(
            "{}:{} unsupported crabenv sink scope `{}` for format={}; use public, private, or all",
            path.display(),
            line_number,
            other,
            format
        ),
    }
}

impl SinkScope {
    fn matches_record_scope(self, scope: &Scope) -> bool {
        match self {
            SinkScope::Public => *scope == Scope::Public,
            SinkScope::Private => *scope == Scope::Private,
            SinkScope::All => matches!(scope, Scope::Public | Scope::Private),
        }
    }
}

impl GhaValueSource {
    fn as_str(self) -> &'static str {
        match self {
            GhaValueSource::Vars => "vars",
            GhaValueSource::Secrets => "secrets",
        }
    }

    fn from_namespace(value: &str) -> Option<Self> {
        match value {
            "vars" => Some(GhaValueSource::Vars),
            "secrets" => Some(GhaValueSource::Secrets),
            _ => None,
        }
    }
}

pub(super) fn selected_schema_records(
    graph: &EnvGraph,
    owner: &Path,
    scope: SinkScope,
) -> Vec<SinkRecord> {
    let mut records = graph
        .values()
        .filter(|record| {
            record.owner == owner
                && scope.matches_record_scope(&record.scope)
                && record.surfaces.contains(&EnvSurface::Schema)
        })
        .map(|record| SinkRecord {
            name: record.name.clone(),
            scope: record.scope.clone(),
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| compare_env_names(&left.name, &right.name));
    records
}

pub(super) fn gha_source_for(
    name: &str,
    scope: &Scope,
    overrides: &BTreeMap<String, GhaValueSource>,
) -> GhaValueSource {
    overrides
        .get(name)
        .copied()
        .unwrap_or_else(|| default_gha_source(scope))
}

fn default_gha_source(scope: &Scope) -> GhaValueSource {
    match scope {
        Scope::Private => GhaValueSource::Secrets,
        Scope::Public | Scope::Unknown => GhaValueSource::Vars,
    }
}

pub(super) fn gha_expression(source: GhaValueSource, name: &str) -> String {
    format!("${{{{ {}.{} }}}}", source.as_str(), name)
}

pub(super) fn source_overrides_from_captures(
    lines: &[String],
    line_re: &Regex,
) -> BTreeMap<String, GhaValueSource> {
    let mut overrides = BTreeMap::new();
    for line in lines {
        let Some(captures) = line_re.captures(line) else {
            continue;
        };
        let Some(name) = captures.get(1).map(|capture| capture.as_str()) else {
            continue;
        };
        let Some(namespace) = captures.get(2).map(|capture| capture.as_str()) else {
            continue;
        };
        let Some(reference_name) = captures.get(3).map(|capture| capture.as_str()) else {
            continue;
        };
        if name != reference_name {
            continue;
        }
        let Some(source) = GhaValueSource::from_namespace(namespace) else {
            continue;
        };
        overrides.insert(name.to_string(), source);
    }
    overrides
}

fn leading_whitespace(line: &str) -> String {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests;
