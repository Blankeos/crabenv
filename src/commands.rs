use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapters::{dotenv, python, typescript};
use crate::cli::{AttachArgs, CopyArgs, DoctorArgs, FormatArgs, InitArgs, MutateArgs, RemoveArgs};
use crate::copy_plan::{build_copy_plan, build_root_example_plan};
use crate::discovery::app_workspaces;
use crate::graph::{build_graph, EnvGraph};
use crate::issues::collect_issues;
use crate::models::{
    EnvRecord, EnvSurface, FileWritePlan, Fix, Project, Scope, Severity, VarMutation,
};
use crate::render::{render_doctor_inventory, render_list};
use crate::util::{color, display_rel, normalize_rel_display, validate_var_name};

pub fn run_init(project: &Project, args: InitArgs) -> Result<()> {
    println!("crabenv init");
    println!("root: {}", project.root.display());
    println!(
        "mode: {}",
        if project.is_monorepo {
            "monorepo"
        } else {
            "single app"
        }
    );
    println!();

    for app in app_workspaces(project) {
        println!("- app: {} ({})", display_rel(&app.rel), app.framework);
        println!(
            "  .env.example: {}",
            if dotenv::example_path(app).exists() {
                "present"
            } else {
                "missing"
            }
        );
        if app.framework != "python" {
            println!(
                "  env.private.ts: {}",
                if typescript::private_schema_path(app).exists() {
                    "present"
                } else {
                    "missing"
                }
            );
            println!(
                "  env.public.ts: {}",
                if typescript::public_schema_path(app).exists() {
                    "present"
                } else {
                    "missing"
                }
            );
        }
    }

    if args.fix {
        let doctor_args = DoctorArgs {
            fix: true,
            yes: args.yes,
        };
        run_doctor(project, doctor_args)?;
    }

    Ok(())
}

pub fn run_list(project: &Project) -> Result<()> {
    let graph = build_graph(project)?;
    render_list(&graph);
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

    render_doctor_inventory(&graph);

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
        println!("fix plan:");
        for fix in &fixes {
            println!("- {}", describe_fix(fix));
        }

        if !args.yes {
            println!("rerun with --yes to apply this plan");
            return Ok(());
        }

        apply_fixes(project, &fixes)?;
        println!("applied {} fix(es)", fixes.len());
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
        let private_schema = typescript::private_schema_path(app);
        if private_schema.exists() {
            paths.insert(private_schema);
        }
        let public_schema = typescript::public_schema_path(app);
        if public_schema.exists() {
            paths.insert(public_schema);
        }
    }

    let mut writes = Vec::new();
    for path in paths {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let formatted = if is_typescript_schema(&path, "env.private.ts") {
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
    let apps = select_mutation_apps(project, args.owner.as_deref(), args.shared)?;

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

    for app in &apps {
        dotenv::upsert_example(&dotenv::example_path(app), variable, &env_value)?;

        if app.framework == "python" {
            python::upsert_schema(app, &mutation)?;
        } else {
            typescript::upsert_schema(app, &mutation, &scope)?;
        }
    }

    print_mutation_result(if update { "updated" } else { "added" }, variable, &apps);

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
    let targets = select_remove_targets(project, &args, variable)?;

    for (app, scope) in &targets {
        remove_from_app(app, variable, scope)?;
    }

    let apps = targets.iter().map(|(app, _)| *app).collect::<Vec<_>>();
    print_mutation_result("removed", variable, &apps);
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
    }
}

fn apply_fixes(project: &Project, fixes: &[Fix]) -> Result<()> {
    let mut should_copy = false;
    let mut should_sync_root_example = false;
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

fn select_mutation_apps<'a>(
    project: &'a Project,
    owner: Option<&Path>,
    shared: bool,
) -> Result<Vec<&'a crate::models::Workspace>> {
    if shared || owner.is_some_and(is_shared_owner_alias) {
        let apps = app_workspaces(project).collect::<Vec<_>>();
        if apps.is_empty() {
            bail!("no app owners found");
        }
        return Ok(apps);
    }

    Ok(vec![select_app(project, owner)?])
}

fn select_remove_targets<'a>(
    project: &'a Project,
    args: &RemoveArgs,
    variable: &str,
) -> Result<Vec<(&'a crate::models::Workspace, RemoveScope)>> {
    if args.shared || args.owner.as_deref().is_some_and(is_shared_owner_alias) {
        let records = definition_records_for_variable(project, variable)?;
        if records.is_empty() {
            bail!("{} was not found in any app owner", variable);
        }

        let mut owners = records
            .iter()
            .map(|record| record.owner.clone())
            .collect::<Vec<_>>();
        owners.sort();
        owners.dedup();

        return owners
            .into_iter()
            .map(|owner| {
                let app = select_app(project, Some(owner.as_path()))?;
                Ok((app, RemoveScope::All))
            })
            .collect();
    }

    if let Some(owner) = args.owner.as_deref() {
        let app = select_app(project, Some(owner))?;
        return Ok(vec![(app, RemoveScope::Scope(remove_arg_scope(args)))]);
    }

    let records = definition_records_for_variable(project, variable)?;
    if records.len() == 1 {
        let record = &records[0];
        let app = select_app(project, Some(record.owner.as_path()))?;
        let scope = match record.scope {
            Scope::Private | Scope::Public => RemoveScope::Scope(record.scope.clone()),
            Scope::Unknown => RemoveScope::All,
        };
        return Ok(vec![(app, scope)]);
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

    Ok(vec![(
        select_app(project, None)?,
        RemoveScope::Scope(remove_arg_scope(args)),
    )])
}

fn definition_records_for_variable(project: &Project, variable: &str) -> Result<Vec<EnvRecord>> {
    Ok(build_graph(project)?
        .values()
        .filter(|record| record.name == variable)
        .filter(|record| record_has_schema_surface(record) || record_has_template_surface(record))
        .cloned()
        .collect())
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

fn print_mutation_result(action: &str, variable: &str, apps: &[&crate::models::Workspace]) {
    if apps.len() > 1 {
        println!("{action} {variable} in shared({})", apps.len());
        for app in apps {
            println!("- {}", display_rel(&app.rel));
        }
    } else if let Some(app) = apps.first() {
        println!("{action} {variable} in {}", display_rel(&app.rel));
    }
}

fn is_shared_owner_alias(owner: &Path) -> bool {
    let normalized = normalize_rel_display(owner);
    matches!(
        normalized.to_string_lossy().as_ref(),
        "shared" | "all" | "*"
    )
}
