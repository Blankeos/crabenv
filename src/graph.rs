use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::adapters;
use crate::discovery::app_workspaces;
use crate::models::{EnvRecord, Project, Scope, SourceKind, VarSource};
use crate::util::display_path;

pub type EnvGraph = BTreeMap<(PathBuf, String), EnvRecord>;

pub fn build_graph(project: &Project) -> Result<EnvGraph> {
    let sources = collect_sources(project)?;
    let mut graph = EnvGraph::new();
    let root_local_names = sources
        .iter()
        .filter(|source| {
            project.is_monorepo
                && matches!(source.kind, SourceKind::EnvLocal)
                && source.owner == PathBuf::from(".")
        })
        .map(|source| source.name.clone())
        .collect::<BTreeSet<_>>();

    for source in sources {
        let key = (source.owner.clone(), source.name.clone());
        let record = graph.entry(key).or_insert_with(|| EnvRecord {
            name: source.name.clone(),
            owner: source.owner.clone(),
            scope: Scope::Unknown,
            value_type: None,
            required: None,
            default_value: None,
            example_value: None,
            local_present: false,
            surfaces: BTreeSet::new(),
            surface_sources: BTreeMap::new(),
            sources: BTreeSet::new(),
        });

        if source.scope != Scope::Unknown {
            record.scope = source.scope.clone();
        }
        if source.value_type.is_some() {
            record.value_type = source.value_type.clone();
        }
        if source.required.is_some() {
            record.required = source.required;
        }
        if source.default_value.is_some() {
            record.default_value = source.default_value.clone();
        }
        if matches!(source.kind, SourceKind::EnvExample) {
            record.example_value = source.value.clone();
        }
        if matches!(source.kind, SourceKind::EnvLocal) {
            record.local_present = true;
        }
        let surface = source.kind.surface();
        let source_location = format!("{}:{}", display_path(&source.path), source.line);
        record.surfaces.insert(surface);
        record
            .surface_sources
            .entry(surface)
            .or_default()
            .insert(source_location.clone());
        record.sources.insert(source_location);
    }

    if project.is_monorepo {
        let local = project.root.join(".env");
        if !root_local_names.is_empty() {
            for record in graph.values_mut() {
                if root_local_names.contains(&record.name) {
                    record.local_present = true;
                    let surface = SourceKind::EnvLocal.surface();
                    let source_location = format!("{}:local", display_path(&local));
                    record.surfaces.insert(surface);
                    record
                        .surface_sources
                        .entry(surface)
                        .or_default()
                        .insert(source_location.clone());
                    record.sources.insert(source_location);
                }
            }
        }
    }

    Ok(graph)
}

pub fn collect_sources(project: &Project) -> Result<Vec<VarSource>> {
    let mut sources = Vec::new();
    let workspace_adapters = adapters::workspace_adapters();
    let project_adapters = adapters::project_adapters();

    for workspace in app_workspaces(project) {
        for adapter in &workspace_adapters {
            sources.extend(adapter.collect(project, workspace).with_context(|| {
                format!(
                    "{} adapter failed for {}",
                    adapter.name(),
                    crate::util::display_rel(&workspace.rel)
                )
            })?);
        }
    }

    for adapter in &project_adapters {
        sources.extend(
            adapter
                .collect(project)
                .with_context(|| format!("{} adapter failed", adapter.name()))?,
        );
    }
    Ok(sources)
}
