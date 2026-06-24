use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use cliclack::{confirm, intro, outro, outro_cancel};

use crate::adapters::{dotenv, python, rust, typescript};
use crate::cli::{AttachArgs, CopyArgs, DoctorArgs, FormatArgs, ListArgs, MutateArgs, RemoveArgs};
use crate::copy_plan::{build_copy_plan, build_root_example_plan};
use crate::discovery::app_workspaces;
use crate::graph::{build_graph, EnvGraph};
use crate::issues::collect_issues;
use crate::models::{
    EnvRecord, EnvSurface, FileWritePlan, Fix, Project, Scope, Severity, VarMutation, Workspace,
};
use crate::render::{render_doctor_inventory, render_list};
use crate::sinks::apply_sink_plan;
use crate::util::{color, display_rel, normalize_rel_display, validate_var_name};

pub fn run_init(project: &Project) -> Result<()> {
    if should_use_cliclack() {
        intro("Initialize crabenv")?;
    } else {
        println!("crabenv init");
    }
    println!("{}  {}", color("Root", "90"), project.root.display());
    println!(
        "{}  {}",
        color("Mode", "90"),
        if project.is_monorepo {
            "monorepo"
        } else {
            "single app"
        }
    );

    let apps = app_workspaces(project).collect::<Vec<_>>();
    if apps.is_empty() {
        println!();
        println!("{} no app owners found", color("!", "33"));
        println!(
            "hint: add a workspace under apps/ or create an env surface, then rerun crabenv init"
        );
        init_outro_cancel("Nothing to initialize.")?;
        return Ok(());
    }

    let mut surface_writes = Vec::<FileWritePlan>::new();
    for app in &apps {
        collect_init_app_writes(app, &mut surface_writes)?;
    }
    dedupe_file_writes(&mut surface_writes);

    let local_write_labels = planned_local_write_labels(project);

    println!();
    println!("{}", color("Detected", "1"));
    for app in &apps {
        println!(
            "  {} {}  {}",
            color("●", "36"),
            display_rel(&app.rel),
            color(format!("({})", app.framework), "90")
        );
        println!(
            "    {:<9} {}",
            color("template", "90"),
            init_status(&dotenv::example_path(app))
        );
        println!("    {:<9} {}", color("schema", "90"), schema_status(app));
    }

    println!();
    if surface_writes.is_empty() && local_write_labels.is_empty() {
        init_outro(format!(
            "crabenv init: {}",
            color("already initialized", "32")
        ))?;
        return Ok(());
    }

    println!("{}", color("Plan", "1"));
    for write in &surface_writes {
        let path = display_path_from_root(project, &write.path)
            .display()
            .to_string();
        println!("  {} {}", color("create", "32"), color(path, "97"));
    }
    for label in &local_write_labels {
        println!(
            "  {} {}",
            color("create", "32"),
            color(label.display().to_string(), "97")
        );
    }

    if !should_use_cliclack() {
        println!(
            "{} run crabenv init in an interactive terminal to create these files",
            color("hint:", "90")
        );
        return Ok(());
    }

    println!();
    let yes = confirm("Create the planned files?").interact()?;
    if !yes {
        init_outro_cancel("Cancelled.")?;
        return Ok(());
    }

    for write in &surface_writes {
        write_init_file(&write)?;
    }
    write_init_local_files(project)?;

    init_outro("Initialized crabenv ✓")?;

    Ok(())
}

fn should_use_cliclack() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn init_outro(message: impl ToString) -> Result<()> {
    let message = message.to_string();
    if should_use_cliclack() {
        outro(message)?;
    } else {
        println!("{message}");
    }
    Ok(())
}

fn init_outro_cancel(message: impl ToString) -> Result<()> {
    let message = message.to_string();
    if should_use_cliclack() {
        outro_cancel(message)?;
    } else {
        println!("{message}");
    }
    Ok(())
}

fn collect_init_app_writes(app: &Workspace, writes: &mut Vec<FileWritePlan>) -> Result<()> {
    push_missing_file(
        writes,
        dotenv::example_path(app),
        "# Add non-secret example values here. crabenv copy uses this file to create .env.\n",
    );

    match app.framework.as_str() {
        "python" => push_missing_file(writes, python_init_schema_path(app), PYTHON_ENV_TEMPLATE),
        "rust" => push_missing_file(writes, rust::config_path(app), RUST_CONFIG_TEMPLATE),
        _ => collect_typescript_init_writes(app, writes),
    }

    Ok(())
}

fn collect_typescript_init_writes(app: &Workspace, writes: &mut Vec<FileWritePlan>) {
    let plain = typescript::plain_schema_path(app);
    let private = typescript::private_schema_path(app);
    let public = typescript::public_schema_path(app);

    if plain.exists() {
        return;
    }

    push_missing_file(writes, private, TYPESCRIPT_PRIVATE_ENV_TEMPLATE);
    push_missing_file(writes, public, TYPESCRIPT_PUBLIC_ENV_TEMPLATE);
}

fn push_missing_file(writes: &mut Vec<FileWritePlan>, path: PathBuf, contents: impl Into<String>) {
    if !path.exists() {
        writes.push(FileWritePlan {
            path,
            contents: contents.into(),
        });
    }
}

fn dedupe_file_writes(writes: &mut Vec<FileWritePlan>) {
    let mut seen = BTreeSet::new();
    writes.retain(|write| seen.insert(write.path.clone()));
}

fn planned_local_write_labels(project: &Project) -> Vec<PathBuf> {
    let mut labels = Vec::new();
    if !project.root.join(".env").exists() {
        labels.push(PathBuf::from(".env"));
    }
    if project.is_monorepo && !project.root.join(".env.example").exists() {
        labels.push(PathBuf::from(".env.example"));
    }
    labels
}

fn write_init_local_files(project: &Project) -> Result<()> {
    if !project.root.join(".env").exists() {
        let plan = build_copy_plan(project, true, false)?;
        for write in plan.writes {
            if write.path.exists() {
                continue;
            }
            write_init_file(&write)?;
        }
    } else if project.is_monorepo && !project.root.join(".env.example").exists() {
        if let Some(plan) = build_root_example_plan(project)? {
            write_init_file(&plan)?;
        }
    }
    Ok(())
}

fn write_init_file(write: &FileWritePlan) -> Result<()> {
    if let Some(parent) = write.path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&write.path, &write.contents)
        .with_context(|| format!("failed to write {}", write.path.display()))?;
    println!("wrote {}", write.path.display());
    Ok(())
}

fn init_status(path: &Path) -> String {
    if path.exists() {
        color("present", "32")
    } else {
        color("will create", "33")
    }
}

fn schema_status(app: &Workspace) -> String {
    match app.framework.as_str() {
        "python" => status_for_path(&python_init_schema_path(app), "src/env.py"),
        "rust" => status_for_path(&rust::config_path(app), "src/config.rs"),
        _ if typescript::should_use_plain_schema(app) => {
            format!("plain src/env.ts {}", color("present", "32"))
        }
        _ if typescript::private_schema_path(app).exists()
            || typescript::public_schema_path(app).exists() =>
        {
            let private = if typescript::private_schema_path(app).exists() {
                color("present", "32")
            } else {
                color("will create", "33")
            };
            let public = if typescript::public_schema_path(app).exists() {
                color("present", "32")
            } else {
                color("will create", "33")
            };
            format!("split private {private}, public {public}")
        }
        _ => format!(
            "split private {}, public {}",
            color("will create", "33"),
            color("will create", "33")
        ),
    }
}

fn status_for_path(path: &Path, label: &str) -> String {
    format!(
        "{label} {}",
        if path.exists() {
            color("present", "32")
        } else {
            color("will create", "33")
        }
    )
}

fn python_init_schema_path(app: &Workspace) -> PathBuf {
    python::find_env_file(&app.root).unwrap_or_else(|| app.root.join("src/env.py"))
}

const TYPESCRIPT_PRIVATE_ENV_TEMPLATE: &str = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {},
});
"#;

const TYPESCRIPT_PUBLIC_ENV_TEMPLATE: &str = r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  client: {},
  runtimeEnvStrict: {},
});
"#;

const PYTHON_ENV_TEMPLATE: &str = r#"from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Env(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")


env = Env()
"#;

const RUST_CONFIG_TEMPLATE: &str = r#"use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {}

impl Config {
    pub fn from_env() -> envy::Result<Self> {
        envy::from_env::<Self>()
    }
}
"#;

pub fn run_list(project: &Project, args: ListArgs) -> Result<()> {
    let graph = build_graph(project)?;
    render_list(project, &graph, args.expand);
    Ok(())
}

pub fn run_doctor(project: &Project, args: DoctorArgs) -> Result<()> {
    let graph = build_graph(project)?;
    let issues = collect_issues(project, &graph)?;
    if issues.is_empty() {
        println!("crabenv doctor: {}", color("no issues found", "32"));
    } else {
        println!("crabenv doctor: {} issue(s)", issues.len());
        for issue in &issues {
            println!("{} {}", severity_label(&issue.severity), issue.message);
            if issue.fix.is_some() {
                println!("      {}", color("fixable", "32"));
            }
        }
    }

    render_doctor_inventory(project, &graph);

    if args.fix {
        let mut fixes = Vec::new();
        for issue in &issues {
            let Some(fix) = issue.fix.clone() else {
                continue;
            };
            if !fixes.contains(&fix) {
                fixes.push(fix);
            }
        }
        if fixes.is_empty() {
            println!("no automatic fixes available");
            return Ok(());
        }

        println!();
        render_fix_plan(&fixes, args.yes);

        if !args.yes {
            return Ok(());
        }

        apply_fixes(project, &fixes)?;
        println!("{} applied {} fix(es)", color("✓", "32"), fixes.len());
    }

    Ok(())
}

pub fn run_format(project: &Project, args: FormatArgs) -> Result<()> {
    let writes = build_format_plan(project)?;
    if writes.is_empty() {
        println!("crabenv format: already formatted");
        return Ok(());
    }

    if args.check {
        println!("crabenv format: {} file(s) would change", writes.len());
        for write in &writes {
            println!(
                "would format {}",
                display_rel(&display_path_from_root(project, &write.path))
            );
        }
        return Ok(());
    }

    for write in &writes {
        fs::write(&write.path, &write.contents)
            .with_context(|| format!("failed to write {}", write.path.display()))?;
        println!(
            "formatted {}",
            display_rel(&display_path_from_root(project, &write.path))
        );
    }
    println!("crabenv format: formatted {} file(s)", writes.len());
    Ok(())
}

fn severity_label(severity: &Severity) -> String {
    let label = format!("[{}]", severity.as_str());
    match severity {
        Severity::Info => color(label, "36"),
        Severity::Warn => color(label, "33"),
        Severity::Error => color(label, "31"),
    }
}

fn render_fix_plan(fixes: &[Fix], apply_now: bool) {
    println!(
        "{} {}",
        color("Fix plan", "1"),
        color(format!("{} automatic fix(es)", fixes.len()), "90")
    );
    for fix in fixes {
        println!(
            "  {} {} {}",
            color("•", fix_accent_color(fix)),
            color(fix_badge(fix), fix_accent_color(fix)),
            describe_fix(fix)
        );
    }

    if apply_now {
        println!(
            "  {} {}",
            color("→", "32"),
            color("applying now because --yes was provided", "90")
        );
    } else {
        println!(
            "  {} {}",
            color("→", "33"),
            color("preview only; rerun with --yes to apply", "90")
        );
    }
}

fn fix_badge(fix: &Fix) -> &'static str {
    match fix {
        Fix::BackfillExample { .. } => "[template]",
        Fix::CreateLocalEnv => "[local]",
        Fix::SyncSinks => "[sinks]",
    }
}

fn fix_accent_color(fix: &Fix) -> &'static str {
    match fix {
        Fix::BackfillExample { .. } => "36",
        Fix::CreateLocalEnv => "33",
        Fix::SyncSinks => "35",
    }
}

fn build_format_plan(project: &Project) -> Result<Vec<FileWritePlan>> {
    let mut paths = BTreeSet::<PathBuf>::new();

    let root_env = project.root.join(".env");
    if root_env.exists() {
        paths.insert(root_env);
    }
    let root_example = project.root.join(".env.example");
    if root_example.exists() {
        paths.insert(root_example);
    }

    for app in app_workspaces(project) {
        let example = dotenv::example_path(app);
        if example.exists() {
            paths.insert(example);
        }
        if !project.is_monorepo {
            let local = dotenv::local_path(project, app);
            if local.exists() {
                paths.insert(local);
            }
        }
        for (schema_path, _) in typescript::active_schema_paths(app) {
            if schema_path.exists() {
                paths.insert(schema_path);
            }
        }
    }

    let mut writes = Vec::new();
    for path in paths {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let formatted = if is_typescript_schema(&path, "env.ts") {
            typescript::format_schema_contents(&contents, &Scope::Private).and_then(|contents| {
                typescript::format_schema_contents(&contents, &Scope::Public)
            })?
        } else if is_typescript_schema(&path, "env.private.ts") {
            typescript::format_schema_contents(&contents, &Scope::Private)?
        } else if is_typescript_schema(&path, "env.public.ts") {
            typescript::format_schema_contents(&contents, &Scope::Public)?
        } else {
            dotenv::format_contents(&contents)
        };

        if formatted != contents {
            writes.push(FileWritePlan {
                path,
                contents: formatted,
            });
        }
    }

    Ok(writes)
}

fn is_typescript_schema(path: &Path, file_name: &str) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(file_name)
}

fn display_path_from_root(project: &Project, path: &Path) -> PathBuf {
    path.strip_prefix(&project.root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub fn run_copy(project: &Project, args: CopyArgs) -> Result<()> {
    let plan = build_copy_plan(project, !args.no_templates, args.overwrite)?;

    if args.dry_run {
        for write in &plan.writes {
            println!("would write {}", write.path.display());
            println!("{}", write.contents);
        }
        return Ok(());
    }

    apply_copy_plan(&plan)?;

    Ok(())
}

fn apply_copy_plan(plan: &crate::models::CopyPlan) -> Result<()> {
    for write in &plan.writes {
        fs::write(&write.path, &write.contents)
            .with_context(|| format!("failed to write {}", write.path.display()))?;
        println!("wrote {}", write.path.display());
    }

    Ok(())
}

fn apply_file_write(write: &crate::models::FileWritePlan) -> Result<()> {
    fs::write(&write.path, &write.contents)
        .with_context(|| format!("failed to write {}", write.path.display()))?;
    println!("wrote {}", write.path.display());
    Ok(())
}

pub fn run_add_or_update(project: &Project, args: MutateArgs, update: bool) -> Result<()> {
    // If no variable was provided, launch the interactive wizard.
    let args = if args.variable.is_none() {
        crate::prompt::prompt_add_or_update(project, update)?
    } else {
        args
    };

    let variable = args.variable.as_deref().unwrap_or("");
    validate_var_name(variable)?;
    let selection = select_mutation_apps(project, args.owner.as_deref(), args.shared.as_deref())?;

    let scope = if args.public {
        Scope::Public
    } else {
        Scope::Private
    };
    let env_value = args
        .example
        .clone()
        .or_else(|| args.default_value.clone())
        .unwrap_or_default();
    let mutation = VarMutation {
        variable: variable.to_string(),
        ..VarMutation::from(&args)
    };

    for app in &selection.apps {
        dotenv::upsert_example(&dotenv::example_path(app), variable, &env_value)?;

        if app.framework == "python" {
            python::upsert_schema(app, &mutation)?;
        } else if app.framework == "rust" {
            rust::upsert_schema(app, &mutation)?;
        } else {
            typescript::upsert_schema(app, &mutation, &scope)?;
        }
    }

    print_mutation_result(
        if update { "updated" } else { "added" },
        variable,
        &selection.apps,
        &selection.target,
    );

    if update {
        sync_root_example(project)?;
    } else {
        sync_local_env(project)?;
    }

    Ok(())
}

pub fn run_attach(project: &Project, args: AttachArgs) -> Result<()> {
    // If no variable was provided, launch the interactive wizard.
    let args = if args.variable.is_none() {
        crate::prompt::prompt_attach(project)?
    } else {
        args
    };

    let variable = args.variable.as_deref().unwrap_or("");
    let owner = args
        .owner
        .as_deref()
        .ok_or_else(|| anyhow!("--owner is required (or run without args for interactive mode)"))?;

    validate_var_name(variable)?;
    let target = select_app(project, Some(owner))?;
    let graph = build_graph(project)?;
    let source = select_attach_source(&graph, variable, args.from.as_deref())?;
    let source_owner = select_app(project, Some(source.owner.as_path()))?;
    let scope = source.scope.clone();
    if scope == Scope::Unknown {
        bail!("{} has no schema scope to attach", variable);
    }

    let example_value = source
        .example_value
        .clone()
        .or_else(|| example_value_for_name(&graph, variable))
        .or_else(|| source.default_value.clone())
        .unwrap_or_default();

    let mutation = mutation_from_record(source);
    let ts_expr = if source.owner == source_owner.rel {
        typescript::read_schema_expr(source_owner, variable, &scope)?
    } else {
        None
    };

    dotenv::upsert_example(&dotenv::example_path(target), variable, &example_value)?;

    if target.framework == "python" {
        python::upsert_schema(target, &mutation)?;
    } else if target.framework == "rust" {
        rust::upsert_schema(target, &mutation)?;
    } else if let Some(expr) = ts_expr {
        typescript::upsert_schema_expr(target, variable, &scope, &expr)?;
    } else {
        typescript::upsert_schema(target, &mutation, &scope)?;
    }

    println!(
        "attached {} from {} to {}",
        variable,
        display_rel(&source.owner),
        display_rel(&target.rel)
    );

    sync_local_env(project)?;

    Ok(())
}

pub fn run_remove(project: &Project, args: RemoveArgs) -> Result<()> {
    // If no variable was provided, launch the interactive wizard.
    let args = if args.variable.is_none() {
        crate::prompt::prompt_remove(project)?
    } else {
        args
    };

    let variable = args.variable.as_deref().unwrap_or("");
    validate_var_name(variable)?;
    let selection = select_remove_targets(project, &args, variable)?;

    for (app, scope) in &selection.targets {
        remove_from_app(app, variable, scope)?;
    }

    let apps = selection
        .targets
        .iter()
        .map(|(app, _)| *app)
        .collect::<Vec<_>>();
    print_mutation_result("removed", variable, &apps, &selection.target);
    sync_root_example(project)?;
    Ok(())
}

fn select_attach_source<'a>(
    graph: &'a EnvGraph,
    variable: &str,
    owner: Option<&Path>,
) -> Result<&'a EnvRecord> {
    let mut matches = graph
        .values()
        .filter(|record| record.name == variable)
        .filter(|record| record_has_schema_surface(record))
        .filter(|record| owner.is_none_or(|owner| record.owner == normalize_rel_display(owner)))
        .collect::<Vec<_>>();

    if matches.is_empty() {
        if let Some(owner) = owner {
            bail!(
                "{} was not found in schema for {}",
                variable,
                display_rel(owner)
            );
        }
        bail!("{variable} was not found in any schema");
    }

    if matches.len() > 1 {
        let owners = matches
            .iter()
            .map(|record| display_rel(&record.owner))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("{variable} exists in multiple owners ({owners}); pass --from apps/name");
    }

    Ok(matches.remove(0))
}

fn record_has_schema_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Schema)
}

fn record_has_template_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Template)
}

fn example_value_for_name(graph: &EnvGraph, variable: &str) -> Option<String> {
    graph
        .values()
        .find(|record| record.name == variable && record.example_value.is_some())
        .and_then(|record| record.example_value.clone())
}

fn mutation_from_record(record: &EnvRecord) -> VarMutation {
    let value_type = record.value_type.as_deref().unwrap_or("string");
    VarMutation {
        variable: record.name.clone(),
        description: record.description.clone(),
        example: record.example_value.clone(),
        optional: record.required == Some(false) && record.default_value.is_none(),
        default_value: record.default_value.clone(),
        numeric: value_type == "regex",
        number: value_type == "number",
        boolean: value_type == "boolean",
        enum_values: None,
        test_regex: None,
        test_regex_message: None,
    }
}

fn describe_fix(fix: &Fix) -> String {
    match fix {
        Fix::BackfillExample { app, name } => {
            format!("add {name} to {}/.env.example", display_rel(app))
        }
        Fix::CreateLocalEnv => "create/update local .env from .env.example files".to_string(),
        Fix::SyncSinks => "sync managed sink blocks".to_string(),
    }
}

fn apply_fixes(project: &Project, fixes: &[Fix]) -> Result<()> {
    let mut should_copy = false;
    let mut should_sync_root_example = false;
    let mut should_sync_sinks = false;
    for fix in fixes {
        match fix {
            Fix::BackfillExample { app, name } => {
                let workspace = app_workspaces(project)
                    .find(|workspace| workspace.rel == *app)
                    .ok_or_else(|| anyhow!("unknown app {}", app.display()))?;
                dotenv::upsert_example(&dotenv::example_path(workspace), name, "")?;
                should_sync_root_example = true;
            }
            Fix::CreateLocalEnv => {
                should_copy = true;
            }
            Fix::SyncSinks => {
                should_sync_sinks = true;
            }
        }
    }

    if should_sync_sinks {
        let graph = build_graph(project)?;
        let plan = apply_sink_plan(project, &graph)?;
        if !plan.writes.is_empty() {
            println!("synced {} sink file(s)", plan.writes.len());
        }
    }

    if should_copy {
        run_copy(
            project,
            CopyArgs {
                dry_run: false,
                overwrite: false,
                no_templates: false,
            },
        )?;
    } else if should_sync_root_example {
        sync_root_example(project)?;
    }
    Ok(())
}

fn sync_local_env(project: &Project) -> Result<()> {
    let plan = build_copy_plan(project, true, false)?;
    apply_copy_plan(&plan)
}

fn sync_root_example(project: &Project) -> Result<()> {
    if let Some(plan) = build_root_example_plan(project)? {
        apply_file_write(&plan)?;
    }
    Ok(())
}

fn select_app<'a>(
    project: &'a Project,
    owner: Option<&Path>,
) -> Result<&'a crate::models::Workspace> {
    let apps = app_workspaces(project).collect::<Vec<_>>();
    if let Some(owner) = owner {
        let normalized = normalize_rel_display(owner);
        return apps
            .into_iter()
            .find(|workspace| workspace.rel == normalized)
            .ok_or_else(|| anyhow!("unknown app owner {}", owner.display()));
    }

    if apps.len() == 1 {
        return Ok(apps[0]);
    }

    bail!("multiple apps found; pass --owner apps/name");
}

#[derive(Clone, Debug)]
enum RemoveScope {
    Scope(Scope),
    All,
}

#[derive(Clone, Debug)]
enum SharedTarget {
    All,
    Selected(Vec<PathBuf>),
}

#[derive(Debug)]
enum MutationTarget {
    Single,
    Shared(SharedTarget),
}

#[derive(Debug)]
struct MutationSelection<'a> {
    apps: Vec<&'a Workspace>,
    target: MutationTarget,
}

#[derive(Debug)]
struct RemoveSelection<'a> {
    targets: Vec<(&'a Workspace, RemoveScope)>,
    target: MutationTarget,
}

fn select_mutation_apps<'a>(
    project: &'a Project,
    owner: Option<&Path>,
    shared: Option<&[PathBuf]>,
) -> Result<MutationSelection<'a>> {
    if let Some(shared) = shared_target_from_args(shared) {
        let apps = select_shared_apps(project, &shared)?;
        return Ok(MutationSelection {
            apps,
            target: MutationTarget::Shared(shared),
        });
    }

    if let Some(owner) = owner.filter(|owner| is_shared_owner_alias(owner)) {
        let shared = shared_target_from_owner_alias(owner);
        let apps = select_shared_apps(project, &shared)?;
        return Ok(MutationSelection {
            apps,
            target: MutationTarget::Shared(shared),
        });
    }

    Ok(MutationSelection {
        apps: vec![select_app(project, owner)?],
        target: MutationTarget::Single,
    })
}

fn select_remove_targets<'a>(
    project: &'a Project,
    args: &RemoveArgs,
    variable: &str,
) -> Result<RemoveSelection<'a>> {
    if let Some(shared) = shared_target_from_args(args.shared.as_deref()).or_else(|| {
        args.owner
            .as_deref()
            .filter(|owner| is_shared_owner_alias(owner))
            .map(shared_target_from_owner_alias)
    }) {
        let records = definition_records_for_variable(project, variable)?;
        if records.is_empty() {
            bail!("{} was not found in any app owner", variable);
        }

        let owners = match &shared {
            SharedTarget::All => unique_record_owners(&records),
            SharedTarget::Selected(owners) => {
                let existing_owners = unique_record_owners(&records)
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                for owner in owners {
                    if !existing_owners.contains(owner) {
                        bail!("{} is not defined in {}", variable, display_rel(owner));
                    }
                }
                owners.clone()
            }
        };

        let targets = owners
            .into_iter()
            .map(|owner| {
                let app = select_app(project, Some(owner.as_path()))?;
                Ok((app, RemoveScope::All))
            })
            .collect::<Result<Vec<_>>>()?;

        return Ok(RemoveSelection {
            targets,
            target: MutationTarget::Shared(shared),
        });
    }

    if let Some(owner) = args.owner.as_deref() {
        let app = select_app(project, Some(owner))?;
        return Ok(RemoveSelection {
            targets: vec![(app, RemoveScope::Scope(remove_arg_scope(args)))],
            target: MutationTarget::Single,
        });
    }

    let records = definition_records_for_variable(project, variable)?;
    if records.len() == 1 {
        let record = &records[0];
        let app = select_app(project, Some(record.owner.as_path()))?;
        let scope = match record.scope {
            Scope::Private | Scope::Public => RemoveScope::Scope(record.scope.clone()),
            Scope::Unknown => RemoveScope::All,
        };
        return Ok(RemoveSelection {
            targets: vec![(app, scope)],
            target: MutationTarget::Single,
        });
    }

    if records.len() > 1 {
        let owners = records
            .iter()
            .map(|record| display_rel(&record.owner))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "{} exists in multiple owners ({owners}); pass --owner apps/name to remove one, or --shared to remove all",
            variable
        );
    }

    Ok(RemoveSelection {
        targets: vec![(
            select_app(project, None)?,
            RemoveScope::Scope(remove_arg_scope(args)),
        )],
        target: MutationTarget::Single,
    })
}

fn definition_records_for_variable(project: &Project, variable: &str) -> Result<Vec<EnvRecord>> {
    Ok(build_graph(project)?
        .values()
        .filter(|record| record.name == variable)
        .filter(|record| record_has_schema_surface(record) || record_has_template_surface(record))
        .cloned()
        .collect())
}

fn shared_target_from_args(shared: Option<&[PathBuf]>) -> Option<SharedTarget> {
    shared.map(|owners| {
        if owners.is_empty() || owners.iter().any(|owner| is_shared_owner_alias(owner)) {
            SharedTarget::All
        } else {
            SharedTarget::Selected(normalize_unique_owners(owners))
        }
    })
}

fn shared_target_from_owner_alias(owner: &Path) -> SharedTarget {
    debug_assert!(is_shared_owner_alias(owner));
    SharedTarget::All
}

fn normalize_unique_owners(owners: &[PathBuf]) -> Vec<PathBuf> {
    let mut normalized = owners
        .iter()
        .map(|owner| normalize_rel_display(owner))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn select_shared_apps<'a>(
    project: &'a Project,
    target: &SharedTarget,
) -> Result<Vec<&'a Workspace>> {
    match target {
        SharedTarget::All => {
            let apps = app_workspaces(project).collect::<Vec<_>>();
            if apps.is_empty() {
                bail!("no app owners found");
            }
            Ok(apps)
        }
        SharedTarget::Selected(owners) => {
            if owners.is_empty() {
                bail!("pass at least one app owner to --shared, or use --shared '*' for all");
            }
            owners
                .iter()
                .map(|owner| select_app(project, Some(owner.as_path())))
                .collect()
        }
    }
}

fn unique_record_owners(records: &[EnvRecord]) -> Vec<PathBuf> {
    let mut owners = records
        .iter()
        .map(|record| record.owner.clone())
        .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    owners
}

fn remove_from_app(
    app: &crate::models::Workspace,
    variable: &str,
    scope: &RemoveScope,
) -> Result<()> {
    dotenv::remove_key(&dotenv::example_path(app), variable)?;

    if app.framework == "python" {
        python::remove_schema(app, variable)?;
        return Ok(());
    }

    if app.framework == "rust" {
        rust::remove_schema(app, variable)?;
        return Ok(());
    }

    match scope {
        RemoveScope::Scope(scope) => typescript::remove_schema(app, variable, scope)?,
        RemoveScope::All => {
            typescript::remove_schema(app, variable, &Scope::Private)?;
            typescript::remove_schema(app, variable, &Scope::Public)?;
        }
    }

    Ok(())
}

fn remove_arg_scope(args: &RemoveArgs) -> Scope {
    if args.public {
        Scope::Public
    } else {
        Scope::Private
    }
}

fn print_mutation_result(
    action: &str,
    variable: &str,
    apps: &[&Workspace],
    target: &MutationTarget,
) {
    if apps.len() > 1 || matches!(target, MutationTarget::Shared(_)) {
        println!(
            "{action} {variable} in {}",
            format_mutation_target(apps, target)
        );
        for app in apps {
            println!("- {}", display_rel(&app.rel));
        }
    } else if let Some(app) = apps.first() {
        println!("{action} {variable} in {}", display_rel(&app.rel));
    }
}

fn format_mutation_target(apps: &[&Workspace], target: &MutationTarget) -> String {
    match target {
        MutationTarget::Single => apps
            .first()
            .map(|app| display_rel(&app.rel))
            .unwrap_or_else(|| "-".to_string()),
        MutationTarget::Shared(SharedTarget::All) => "shared(all)".to_string(),
        MutationTarget::Shared(SharedTarget::Selected(owners)) => {
            let owners = if owners.is_empty() {
                apps.iter().map(|app| app.rel.clone()).collect::<Vec<_>>()
            } else {
                owners.clone()
            };
            format!(
                "shared({})",
                owners
                    .iter()
                    .map(|owner| display_rel(owner))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn is_shared_owner_alias(owner: &Path) -> bool {
    let normalized = normalize_rel_display(owner);
    matches!(
        normalized.to_string_lossy().as_ref(),
        "shared" | "all" | "*"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_target_from_args_treats_empty_and_star_as_all() {
        assert!(matches!(
            shared_target_from_args(Some(&[])),
            Some(SharedTarget::All)
        ));

        let owners = vec![PathBuf::from("*")];
        assert!(matches!(
            shared_target_from_args(Some(&owners)),
            Some(SharedTarget::All)
        ));
    }

    #[test]
    fn shared_target_from_args_normalizes_and_dedupes_selected_owners() {
        let owners = vec![
            PathBuf::from("apps/web"),
            PathBuf::from("apps/api"),
            PathBuf::from("apps/web"),
        ];

        let Some(SharedTarget::Selected(selected)) = shared_target_from_args(Some(&owners)) else {
            panic!("expected selected shared owners");
        };

        assert_eq!(
            selected,
            vec![PathBuf::from("apps/api"), PathBuf::from("apps/web")]
        );
    }

    #[test]
    fn shared_owner_aliases_stay_backward_compatible_all_targets() {
        for owner in ["shared", "all", "*"] {
            assert!(is_shared_owner_alias(Path::new(owner)));
            assert!(matches!(
                shared_target_from_owner_alias(Path::new(owner)),
                SharedTarget::All
            ));
        }
    }
}
