//! Interactive prompt helpers built on cliclack.
//!
//! Each command that supports interactivity follows the same pattern:
//! if the user did not supply enough arguments on the CLI, fall back
//! to a guided wizard with `intro` / `outro` / `outro_cancel`.

use anyhow::{bail, Result};
use cliclack::{confirm, input, intro, multiselect, outro, outro_cancel, select};
use colored::Colorize;
use std::path::PathBuf;

use crate::cli::{AttachArgs, MutateArgs, RemoveArgs};
use crate::discovery::app_workspaces;
use crate::graph::{build_graph, EnvGraph};
use crate::models::{EnvRecord, EnvSurface, Project, Scope, Workspace};
use crate::ordering::sort_env_names;
use crate::util::{display_rel, is_valid_var_name};

// ---------------------------------------------------------------------------
// Public entry points — called from commands.rs when a command runs bare.
// ---------------------------------------------------------------------------

/// Interactive wizard for `crabenv add` and `crabenv update`.
/// Returns a fully-populated `MutateArgs`.
pub fn prompt_add_or_update(project: &Project, update: bool) -> Result<MutateArgs> {
    intro(if update {
        "Update an env var"
    } else {
        "Add an env var"
    })?;

    let graph = if update {
        Some(build_graph(project)?)
    } else {
        None
    };
    let apps = collect_apps(project);
    if apps.is_empty() {
        outro_cancel("No app owners found in this project.")?;
        bail!("no app owners");
    }

    // --- Variable name ---
    let variable = if update {
        prompt_existing_var(graph.as_ref().expect("update graph is present"))?
    } else {
        prompt_var_name()?
    };

    // --- Owner selection ---
    let old_context = graph
        .as_ref()
        .map(|graph| UpdateContext::from_graph(graph, &variable));
    let old_owner = old_context.as_ref().and_then(UpdateContext::owner_hint);
    let (owner, shared) = prompt_owner(&apps, "target", old_owner.as_ref())?;

    let selected_old = old_context
        .as_ref()
        .and_then(|context| context.for_owner(&owner, shared.is_some()));

    // --- Scope (private/public) ---
    let public = prompt_scope(selected_old.as_ref().map(|record| &record.scope))?;

    // --- Type selection ---
    let (numeric, number, boolean, string, enum_values) = prompt_type_bundle(
        selected_old
            .as_ref()
            .and_then(|record| record.value_type.as_deref()),
    )?;

    // --- Description ---
    let description = prompt_description(
        selected_old
            .as_ref()
            .and_then(|record| record.description.as_deref()),
    )?;

    // --- Example value ---
    let placeholder = if number || numeric {
        "e.g. 8787"
    } else if boolean {
        "e.g. true"
    } else {
        "e.g. file:./local.db or http://localhost:8787"
    };
    let example = prompt_example(
        placeholder,
        selected_old
            .as_ref()
            .and_then(|record| record.example_value.as_deref()),
    )?;

    // --- Optional / default ---
    let old_optional = selected_old
        .as_ref()
        .and_then(|record| record.required)
        .map(|required| !required)
        .unwrap_or(false);
    let optional = confirm("Mark as optional?")
        .initial_value(old_optional)
        .interact()?;

    let default_value = if optional {
        let mut prompt = input("Default value (leave empty for none)");
        if let Some(value) = selected_old
            .as_ref()
            .and_then(|record| record.default_value.as_deref())
        {
            prompt = prompt.default_input(value);
        } else {
            prompt = prompt.placeholder("optional default");
        }
        let val: String = prompt.required(false).interact()?;
        if val.trim().is_empty() {
            None
        } else {
            Some(val)
        }
    } else {
        None
    };

    let action = if update { "Update" } else { "Add" };
    let scope_str = if public { "public" } else { "private" };
    let owner_str = if let Some(shared) = &shared {
        format_shared_prompt_target(shared, &apps)
    } else {
        owner.to_string_lossy().to_string()
    };
    let yes = confirm(mutation_confirm_prompt(
        action, &variable, &owner_str, scope_str,
    ))
    .interact()?;

    if !yes {
        outro_cancel("Cancelled.")?;
        bail!("cancelled");
    }

    outro(format!("{} {} ✓", action, variable))?;

    Ok(MutateArgs {
        variable: Some(variable),
        owner: Some(owner),
        shared,
        public,
        example,
        description,
        optional,
        default_value,
        string,
        numeric,
        number,
        boolean,
        enum_values,
        test_regex: None,
        test_regex_message: None,
    })
}

/// Interactive wizard for `crabenv attach`.
/// Returns a fully-populated `AttachArgs`.
pub fn prompt_attach(project: &Project) -> Result<AttachArgs> {
    intro("Attach an env var")?;

    let apps = collect_apps(project);
    if apps.is_empty() {
        outro_cancel("No app owners found in this project.")?;
        bail!("no app owners");
    }

    let graph = build_graph(project)?;

    // --- Variable name (must already exist in some schema) ---
    let variable = prompt_existing_var(&graph)?;

    // --- Source owner (--from) ---
    let candidates = find_var_in_schemas(&graph, &variable);
    let from = if candidates.len() > 1 {
        let labels: Vec<String> = candidates.iter().map(|c| display_rel(c)).collect();
        let choice = prompt_searchable(
            "Copy from which owner?",
            "type to search owners…",
            labels.clone(),
        )?;
        // Map the display label back to the PathBuf
        let idx = labels.iter().position(|l| l == &choice);
        idx.and_then(|i| candidates.get(i).cloned()).or_else(|| {
            candidates
                .iter()
                .find(|c| display_rel(c) == choice)
                .cloned()
        })
    } else {
        candidates.first().cloned()
    };

    // --- Target owner (--owner) ---
    let from_rel = from.as_ref().map(|f| crate::util::normalize_rel_display(f));
    let target_apps: Vec<&Workspace> = apps
        .iter()
        .filter(|a| from_rel.as_ref().is_none_or(|f| &a.rel != f))
        .copied()
        .collect();

    if target_apps.is_empty() {
        outro_cancel("No other owners to attach to.")?;
        bail!("no target owners");
    }

    let owner = if target_apps.len() == 1 {
        target_apps[0].rel.clone()
    } else {
        prompt_select_owner(&target_apps, "target")?
    };

    let from_display = from.as_ref().map(|f| display_rel(f)).unwrap_or_default();
    let yes = confirm(format!(
        "Attach {} from {} to {}?",
        variable.cyan(),
        from_display,
        display_rel(&owner).cyan()
    ))
    .interact()?;

    if !yes {
        outro_cancel("Cancelled.")?;
        bail!("cancelled");
    }

    outro(format!("Attached {} ✓", variable))?;

    Ok(AttachArgs {
        variable: Some(variable),
        from,
        owner: Some(owner),
    })
}

/// Interactive wizard for `crabenv remove`.
/// Returns a fully-populated `RemoveArgs`.
pub fn prompt_remove(project: &Project) -> Result<RemoveArgs> {
    intro("Remove an env var")?;

    let graph = build_graph(project)?;

    // --- Variable name (must exist somewhere) ---
    let variable = prompt_existing_var(&graph)?;

    let candidates = graph
        .values()
        .filter(|r| r.name == variable)
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        outro_cancel(format!("{variable} was not found."))?;
        bail!("not found");
    }

    // --- Owner / shared ---
    let (owner, shared) = if candidates.len() > 1 {
        let choice = select("Remove from which owners?")
            .item(
                "shared",
                format!("All owners ({})", candidates.len()),
                "remove from all or selected owners that define this variable",
            )
            .item(
                "one",
                "A single owner".to_string(),
                "remove from one specific owner",
            )
            .interact()?;

        if choice == "shared" {
            let owners = candidates
                .iter()
                .map(|r| r.owner.clone())
                .collect::<Vec<_>>();
            (
                None,
                Some(prompt_shared_owner_paths(
                    &owners,
                    "Select owners to remove from",
                )?),
            )
        } else {
            let owners = candidates
                .iter()
                .map(|r| r.owner.clone())
                .collect::<Vec<_>>();
            let owner = prompt_select_owners(&owners, "remove from")?;
            (Some(owner), None)
        }
    } else {
        (None, None) // single owner — will be auto-detected
    };

    // --- Scope ---
    let public = if shared.is_some() {
        false
    } else {
        let scopes = candidates
            .iter()
            .filter(|r| {
                owner
                    .as_ref()
                    .is_none_or(|o| r.owner == crate::util::normalize_rel_display(o))
            })
            .map(|r| &r.scope)
            .collect::<Vec<_>>();
        if scopes.iter().any(|s| **s == Scope::Public) {
            let choice = select("Remove from which scope?")
                .item("private", "Private (env.private.ts)", "")
                .item("public", "Public (env.public.ts)", "")
                .item("all", "Both", "remove from both public and private")
                .interact()?;
            choice == "public"
        } else {
            false
        }
    };

    let scope_str = if public { "public" } else { "private" };
    let owner_str = if let Some(shared) = &shared {
        let owners = candidates
            .iter()
            .map(|record| record.owner.clone())
            .collect::<Vec<_>>();
        format_shared_owner_paths(shared, &owners)
    } else if let Some(ref o) = owner {
        display_rel(o)
    } else {
        "auto".to_string()
    };

    let yes = confirm(format!(
        "Remove {} from {} ({scope_str})?",
        variable.cyan(),
        owner_str
    ))
    .interact()?;

    if !yes {
        outro_cancel("Cancelled.")?;
        bail!("cancelled");
    }

    outro(format!("Removed {} ✓", variable))?;

    Ok(RemoveArgs {
        variable: Some(variable),
        owner,
        shared,
        public,
    })
}

// ---------------------------------------------------------------------------
// Internal prompt helpers
// ---------------------------------------------------------------------------

fn collect_apps(project: &Project) -> Vec<&Workspace> {
    app_workspaces(project).collect()
}

#[derive(Clone, Debug)]
struct UpdateContext {
    records: Vec<EnvRecord>,
}

impl UpdateContext {
    fn from_graph(graph: &EnvGraph, variable: &str) -> Self {
        let mut records = graph
            .values()
            .filter(|record| record.name == variable)
            .filter(|record| {
                record.surfaces.contains(&EnvSurface::Schema)
                    || record.surfaces.contains(&EnvSurface::Template)
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|a, b| a.owner.cmp(&b.owner));
        Self { records }
    }

    fn owner_hint(&self) -> Option<PathBuf> {
        if self.records.len() == 1 {
            self.records.first().map(|record| record.owner.clone())
        } else {
            None
        }
    }

    fn for_owner(&self, owner: &PathBuf, shared: bool) -> Option<EnvRecord> {
        if shared {
            return common_record(&self.records);
        }

        self.records
            .iter()
            .find(|record| record.owner == *owner)
            .cloned()
            .or_else(|| common_record(&self.records))
    }
}

fn format_shared_owner_paths(shared: &[PathBuf], all_owners: &[PathBuf]) -> String {
    if shared.is_empty() || shared.len() == unique_paths(all_owners.to_vec()).len() {
        "shared(all)".to_string()
    } else {
        format!(
            "shared({})",
            shared
                .iter()
                .map(|owner| display_rel(owner))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn common_record(records: &[EnvRecord]) -> Option<EnvRecord> {
    let first = records.first()?.clone();
    Some(EnvRecord {
        scope: common_by(records, |record| &record.scope).unwrap_or(Scope::Unknown),
        value_type: common_by(records, |record| &record.value_type).flatten(),
        enum_values: common_by(records, |record| &record.enum_values).flatten(),
        required: common_by(records, |record| &record.required).flatten(),
        default_value: common_by(records, |record| &record.default_value).flatten(),
        description: common_by(records, |record| &record.description).flatten(),
        example_value: common_by(records, |record| &record.example_value).flatten(),
        ..first
    })
}

fn common_by<T: Eq + Clone>(records: &[EnvRecord], value: impl Fn(&EnvRecord) -> &T) -> Option<T> {
    let first = value(records.first()?).clone();
    records
        .iter()
        .all(|record| value(record) == &first)
        .then_some(first)
}

fn scope_choice(scope: &Scope) -> Option<&'static str> {
    match scope {
        Scope::Private => Some("private"),
        Scope::Public => Some("public"),
        Scope::Unknown => None,
    }
}

fn type_choice(value_type: &str) -> Option<&'static str> {
    match value_type {
        "number" => Some("number"),
        "boolean" => Some("boolean"),
        value if value.starts_with("enum") => Some("enum"),
        "string" => Some("string"),
        _ => None,
    }
}

fn type_marker(label: &str, choice: &str, initial: Option<&str>, old_type: Option<&str>) -> String {
    if initial == Some(choice) {
        return match old_type {
            Some(old_type) if old_type != label => format!("{label} (old: {old_type})"),
            _ => old_marker(label, true),
        };
    }

    match (choice, old_type) {
        ("numeric", Some("regex")) => format!("{label} (old: regex)"),
        ("string", Some("url")) => format!("{label} (old: url)"),
        ("string", Some(old_type)) if type_choice(old_type).is_none() && old_type != "regex" => {
            format!("{label} (old: {old_type})")
        }
        _ => label.to_string(),
    }
}

fn old_marker(label: &str, is_old: bool) -> String {
    if is_old {
        format!("{label} (old value)")
    } else {
        label.to_string()
    }
}

fn mutation_confirm_prompt(action: &str, variable: &str, owner: &str, scope: &str) -> String {
    format!("Apply: {action} {variable} in {owner} ({scope})?")
}

/// A filterable select that keeps cliclack's original radio-circle option UI.
///
/// Options are visible immediately, typing fuzzy-filters the list, and the
/// cursor wraps in both directions via our local cliclack patch.
fn prompt_searchable(message: &str, _placeholder: &str, candidates: Vec<String>) -> Result<String> {
    prompt_searchable_with_initial(message, candidates, None)
}

fn prompt_searchable_with_initial(
    message: &str,
    candidates: Vec<String>,
    initial: Option<&str>,
) -> Result<String> {
    if candidates.len() == 1 {
        return Ok(candidates.into_iter().next().unwrap());
    }

    let mut prompt = select(message).filter_mode().max_rows(10);
    for candidate in candidates {
        prompt = prompt.item(candidate.clone(), candidate, "");
    }
    if let Some(initial) = initial {
        prompt = prompt.initial_value(initial.to_string());
    }

    Ok(prompt.interact()?)
}

fn prompt_var_name() -> Result<String> {
    let name: String = input("Variable name")
        .placeholder("e.g. DATABASE_URL")
        .validate(|s: &String| {
            let s = s.trim();
            if s.is_empty() {
                Err("Variable name is required".to_string())
            } else if !is_valid_var_name(s) {
                Err("Must be uppercase ASCII with _ or digits".to_string())
            } else {
                Ok(())
            }
        })
        .interact()?;
    Ok(name.trim().to_uppercase())
}

fn prompt_existing_var(graph: &EnvGraph) -> Result<String> {
    let mut names: Vec<String> = graph
        .values()
        .filter(|r| {
            r.surfaces.contains(&EnvSurface::Schema) || r.surfaces.contains(&EnvSurface::Template)
        })
        .map(|r| r.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    sort_env_names(&mut names);

    if names.is_empty() {
        bail!("no env vars found in any schema or template");
    }

    prompt_searchable("Select a variable", "type to search…", names)
}

fn prompt_owner(
    apps: &[&Workspace],
    label: &str,
    initial_owner: Option<&PathBuf>,
) -> Result<(PathBuf, Option<Vec<PathBuf>>)> {
    if apps.len() == 1 {
        return Ok((apps[0].rel.clone(), None));
    }

    let mut candidates = vec!["Shared".to_string()];
    candidates.extend(apps.iter().map(|app| app.rel.to_string_lossy().to_string()));
    let initial = initial_owner.map(|owner| owner.to_string_lossy().to_string());

    let choice = prompt_searchable_with_initial(
        &format!("Select {label} owner"),
        candidates,
        initial.as_deref(),
    )?;

    if choice == "Shared" {
        let shared = prompt_shared_targets(apps)?;
        Ok((PathBuf::from("."), Some(shared)))
    } else {
        Ok((PathBuf::from(choice), None))
    }
}

fn prompt_shared_targets(apps: &[&Workspace]) -> Result<Vec<PathBuf>> {
    const ALL: &str = "*";

    let mut prompt = multiselect("Select shared app owners")
        .item(
            ALL.to_string(),
            "All (*)".to_string(),
            "apply to every app owner",
        )
        .initial_values(vec![ALL.to_string()])
        .max_rows(10);

    for app in apps {
        let rel = app.rel.to_string_lossy().to_string();
        prompt = prompt.item(rel.clone(), rel, "");
    }

    let selected = prompt.interact()?;
    if selected.iter().any(|value| value == ALL) {
        Ok(Vec::new())
    } else {
        Ok(unique_paths(
            selected.into_iter().map(PathBuf::from).collect(),
        ))
    }
}

fn prompt_shared_owner_paths(owners: &[PathBuf], message: &str) -> Result<Vec<PathBuf>> {
    const ALL: &str = "*";
    let owners = unique_paths(owners.to_vec());

    let mut prompt = multiselect(message)
        .item(
            ALL.to_string(),
            "All (*)".to_string(),
            "use every listed owner",
        )
        .initial_values(vec![ALL.to_string()])
        .max_rows(10);

    for owner in &owners {
        let rel = owner.to_string_lossy().to_string();
        prompt = prompt.item(rel.clone(), rel, "");
    }

    let selected = prompt.interact()?;
    if selected.iter().any(|value| value == ALL) {
        Ok(Vec::new())
    } else {
        Ok(unique_paths(
            selected.into_iter().map(PathBuf::from).collect(),
        ))
    }
}

fn unique_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    paths
}

fn format_shared_prompt_target(shared: &[PathBuf], apps: &[&Workspace]) -> String {
    if shared.is_empty() || shared.len() == apps.len() {
        "shared(all)".to_string()
    } else {
        format!(
            "shared({})",
            shared
                .iter()
                .map(|owner| display_rel(owner))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn prompt_select_owner(apps: &[&Workspace], label: &str) -> Result<PathBuf> {
    let candidates: Vec<String> = apps
        .iter()
        .map(|a| a.rel.to_string_lossy().to_string())
        .collect();
    let choice = prompt_searchable(
        &format!("Select {label} owner"),
        "type to search owners…",
        candidates,
    )?;
    Ok(PathBuf::from(choice))
}

fn prompt_select_owners(owners: &[PathBuf], label: &str) -> Result<PathBuf> {
    let candidates: Vec<String> = owners
        .iter()
        .map(|o| o.to_string_lossy().to_string())
        .collect();
    let choice = prompt_searchable(
        &format!("Select owner to {label}"),
        "type to search owners…",
        candidates,
    )?;
    Ok(PathBuf::from(choice))
}

fn prompt_scope(old_scope: Option<&Scope>) -> Result<bool> {
    let private_label = old_marker(
        "Private (env.private.ts)",
        matches!(old_scope, Some(Scope::Private)),
    );
    let public_label = old_marker(
        "Public (env.public.ts)",
        matches!(old_scope, Some(Scope::Public)),
    );
    let mut prompt = select("Scope")
        .item("private", private_label, "default, server-side only")
        .item("public", public_label, "exposed to client, NEXT_PUBLIC_*");
    if let Some(scope) = old_scope.and_then(scope_choice) {
        prompt = prompt.initial_value(scope);
    }
    let choice = prompt.interact()?;
    Ok(choice == "public")
}

/// Returns (numeric, number, boolean, string, enum_values)
fn prompt_type_bundle(old_type: Option<&str>) -> Result<(bool, bool, bool, bool, Option<String>)> {
    let initial = old_type.and_then(type_choice);
    let mut prompt = select("Type")
        .item(
            "string",
            type_marker("string", "string", initial, old_type),
            "z.string() — any text value",
        )
        .item(
            "number",
            type_marker("number", "number", initial, old_type),
            "z.coerce.number() — parsed as a number",
        )
        .item(
            "numeric",
            type_marker("numeric", "numeric", initial, old_type),
            "numeric string regex — string that looks like a number",
        )
        .item(
            "boolean",
            type_marker("boolean", "boolean", initial, old_type),
            "z.coerce.boolean() — true/false",
        )
        .item(
            "enum",
            type_marker("enum", "enum", initial, old_type),
            "z.enum([...]) — one of several allowed values",
        );
    if let Some(initial) = initial {
        prompt = prompt.initial_value(initial);
    }
    let choice = prompt.interact()?;

    match choice {
        "number" => Ok((false, true, false, false, None)),
        "numeric" => Ok((true, false, false, false, None)),
        "boolean" => Ok((false, false, true, false, None)),
        "enum" => {
            let values: String = input("Enum values (comma-separated)")
                .placeholder("e.g. debug,info,warn,error")
                .validate(|s: &String| {
                    if s.trim().is_empty() {
                        Err("At least one value required".to_string())
                    } else {
                        Ok(())
                    }
                })
                .interact()?;
            Ok((false, false, false, false, Some(values)))
        }
        _ => Ok((false, false, false, true, None)),
    }
}

fn prompt_example(placeholder: &str, old_value: Option<&str>) -> Result<Option<String>> {
    let mut prompt = input("Example value for .env.example (optional)");
    if let Some(old_value) = old_value {
        prompt = prompt.default_input(old_value);
    } else {
        prompt = prompt.placeholder(placeholder).default_input("");
    }
    let value: String = prompt.required(false).interact()?;
    if value.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn prompt_description(old_value: Option<&str>) -> Result<Option<String>> {
    let mut prompt = input("Description (optional)");
    if let Some(old_value) = old_value {
        prompt = prompt.default_input(old_value);
    } else {
        prompt = prompt
            .placeholder("what this env var is used for")
            .default_input("");
    }
    let value: String = prompt.required(false).interact()?;
    if value.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn find_var_in_schemas(graph: &EnvGraph, variable: &str) -> Vec<PathBuf> {
    let mut owners = graph
        .values()
        .filter(|r| r.name == variable && r.surfaces.contains(&EnvSurface::Schema))
        .map(|r| r.owner.clone())
        .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    owners
}
