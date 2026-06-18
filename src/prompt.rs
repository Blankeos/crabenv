//! Interactive prompt helpers built on cliclack.
//!
//! Each command that supports interactivity follows the same pattern:
//! if the user did not supply enough arguments on the CLI, fall back
//! to a guided wizard with `intro` / `outro` / `outro_cancel`.

use anyhow::{bail, Result};
use cliclack::{confirm, input, intro, outro, outro_cancel, select};
use colored::Colorize;
use std::path::PathBuf;

use crate::cli::{AttachArgs, MutateArgs, RemoveArgs};
use crate::discovery::app_workspaces;
use crate::graph::{build_graph, EnvGraph};
use crate::models::{EnvSurface, Project, Scope, Workspace};
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

    let apps = collect_apps(project);
    if apps.is_empty() {
        outro_cancel("No app owners found in this project.")?;
        bail!("no app owners");
    }

    // --- Variable name ---
    let variable = if update {
        let graph = build_graph(project)?;
        prompt_existing_var(&graph)?
    } else {
        prompt_var_name()?
    };

    // --- Owner selection ---
    let (owner, shared) = prompt_owner(&apps, "target")?;

    // --- Scope (private/public) ---
    let public = prompt_scope()?;

    // --- Type selection ---
    let (numeric, number, boolean, string, enum_values) = prompt_type_bundle()?;

    // --- Example value ---
    let placeholder = if number || numeric {
        "e.g. 8787"
    } else if boolean {
        "e.g. true"
    } else {
        "e.g. file:./local.db or http://localhost:8787"
    };
    let example = prompt_example(placeholder)?;

    // --- Optional / default ---
    let optional = confirm("Mark as optional?")
        .initial_value(false)
        .interact()?;

    let default_value = if optional {
        let val: String = input("Default value (leave empty for none)")
            .placeholder("optional default")
            .interact()?;
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
    let owner_str = if shared {
        format!("shared({})", apps.len())
    } else {
        owner.to_string_lossy().to_string()
    };
    let yes = confirm(format!("{action} {variable} in {owner_str} ({scope_str})?")).interact()?;

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
                "remove from every owner that defines this variable",
            )
            .item(
                "one",
                "A single owner".to_string(),
                "remove from one specific owner",
            )
            .interact()?;

        if choice == "shared" {
            (None, true)
        } else {
            let owners = candidates
                .iter()
                .map(|r| r.owner.clone())
                .collect::<Vec<_>>();
            let owner = prompt_select_owners(&owners, "remove from")?;
            (Some(owner), false)
        }
    } else {
        (None, false) // single owner — will be auto-detected
    };

    // --- Scope ---
    let public = if shared {
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
    let owner_str = if shared {
        format!("all owners ({})", candidates.len())
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

/// A filterable select that keeps cliclack's original radio-circle option UI.
///
/// Options are visible immediately, typing fuzzy-filters the list, and the
/// cursor wraps in both directions via our local cliclack patch.
fn prompt_searchable(message: &str, _placeholder: &str, candidates: Vec<String>) -> Result<String> {
    if candidates.len() == 1 {
        return Ok(candidates.into_iter().next().unwrap());
    }

    let mut prompt = select(message).filter_mode().max_rows(10);
    for candidate in candidates {
        prompt = prompt.item(candidate.clone(), candidate, "");
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

fn prompt_owner(apps: &[&Workspace], label: &str) -> Result<(PathBuf, bool)> {
    if apps.len() == 1 {
        return Ok((apps[0].rel.clone(), false));
    }

    let mut candidates = vec!["__shared__".to_string()];
    candidates.extend(apps.iter().map(|a| a.rel.to_string_lossy().to_string()));

    let choice = prompt_searchable(
        &format!("Select {label} owner (or shared)"),
        "type to search owners…",
        candidates,
    )?;

    if choice == "__shared__" {
        Ok((PathBuf::from("."), true))
    } else {
        Ok((PathBuf::from(choice), false))
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

fn prompt_scope() -> Result<bool> {
    let choice = select("Scope")
        .item(
            "private",
            "Private (env.private.ts)",
            "default, server-side only",
        )
        .item(
            "public",
            "Public (env.public.ts)",
            "exposed to client, NEXT_PUBLIC_*",
        )
        .interact()?;
    Ok(choice == "public")
}

/// Returns (numeric, number, boolean, string, enum_values)
fn prompt_type_bundle() -> Result<(bool, bool, bool, bool, Option<String>)> {
    let choice = select("Type")
        .item("string", "string", "z.string() — any text value")
        .item("number", "number", "z.coerce.number() — parsed as a number")
        .item(
            "numeric",
            "numeric",
            "numeric string regex — string that looks like a number",
        )
        .item("boolean", "boolean", "z.coerce.boolean() — true/false")
        .item(
            "enum",
            "enum",
            "z.enum([...]) — one of several allowed values",
        )
        .interact()?;

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

fn prompt_example(placeholder: &str) -> Result<Option<String>> {
    let value: String = input("Example value for .env.example")
        .placeholder(placeholder)
        .interact()?;
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
