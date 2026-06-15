use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::Path;

use crate::adapters::{dotenv, python, typescript};
use crate::cli::{AttachArgs, CopyArgs, DoctorArgs, InitArgs, MutateArgs, RemoveArgs};
use crate::copy_plan::build_copy_plan;
use crate::discovery::app_workspaces;
use crate::graph::{build_graph, EnvGraph};
use crate::issues::collect_issues;
use crate::models::{EnvRecord, EnvSurface, Fix, Project, Scope, Severity, VarMutation};
use crate::render::render_list;
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
    let issues = collect_issues(project)?;
    if issues.is_empty() {
        println!("crabenv doctor: {}", color("no issues found", "32"));
        return Ok(());
    }

    println!("crabenv doctor: {} issue(s)", issues.len());
    for issue in &issues {
        println!("{} {}", severity_label(&issue.severity), issue.message);
        if issue.fix.is_some() {
            println!("      {}", color("fixable", "32"));
        }
    }

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

fn severity_label(severity: &Severity) -> String {
    let label = format!("[{}]", severity.as_str());
    match severity {
        Severity::Info => color(label, "36"),
        Severity::Warn => color(label, "33"),
        Severity::Error => color(label, "31"),
    }
}

pub fn run_copy(project: &Project, args: CopyArgs) -> Result<()> {
    let plan = build_copy_plan(project, !args.no_templates, args.overwrite)?;

    if args.dry_run {
        println!("would write {}", plan.env_path.display());
        println!("{}", plan.env_contents);
        for (path, contents) in &plan.dev_vars {
            println!("would write {}", path.display());
            println!("{}", contents);
        }
        return Ok(());
    }

    apply_copy_plan(&plan)?;

    Ok(())
}

fn apply_copy_plan(plan: &crate::models::CopyPlan) -> Result<()> {
    fs::write(&plan.env_path, &plan.env_contents)
        .with_context(|| format!("failed to write {}", plan.env_path.display()))?;
    println!("wrote {}", plan.env_path.display());

    for (path, contents) in &plan.dev_vars {
        fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

pub fn run_add_or_update(project: &Project, args: MutateArgs, update: bool) -> Result<()> {
    validate_var_name(&args.variable)?;
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
    let mutation = VarMutation::from(&args);

    for app in &apps {
        dotenv::upsert_example(&dotenv::example_path(app), &args.variable, &env_value)?;

        if app.framework == "python" {
            python::upsert_schema(app, &mutation)?;
        } else {
            typescript::upsert_schema(app, &mutation, &scope)?;
        }
    }

    print_mutation_result(
        if update { "updated" } else { "added" },
        &args.variable,
        &apps,
    );

    if !update {
        sync_local_env(project)?;
    }

    Ok(())
}

pub fn run_attach(project: &Project, args: AttachArgs) -> Result<()> {
    validate_var_name(&args.variable)?;
    let target = select_app(project, Some(args.owner.as_path()))?;
    let graph = build_graph(project)?;
    let source = select_attach_source(&graph, &args.variable, args.from.as_deref())?;
    let source_owner = select_app(project, Some(source.owner.as_path()))?;
    let scope = source.scope.clone();
    if scope == Scope::Unknown {
        bail!("{} has no schema scope to attach", args.variable);
    }

    let example_value = source
        .example_value
        .clone()
        .or_else(|| example_value_for_name(&graph, &args.variable))
        .or_else(|| source.default_value.clone())
        .unwrap_or_default();

    let mutation = mutation_from_record(source);
    let ts_expr = if source.owner == source_owner.rel {
        typescript::read_schema_expr(source_owner, &args.variable, &scope)?
    } else {
        None
    };

    dotenv::upsert_example(
        &dotenv::example_path(target),
        &args.variable,
        &example_value,
    )?;

    if target.framework == "python" {
        python::upsert_schema(target, &mutation)?;
    } else if let Some(expr) = ts_expr {
        typescript::upsert_schema_expr(target, &args.variable, &scope, &expr)?;
    } else {
        typescript::upsert_schema(target, &mutation, &scope)?;
    }

    println!(
        "attached {} from {} to {}",
        args.variable,
        display_rel(&source.owner),
        display_rel(&target.rel)
    );

    sync_local_env(project)?;

    Ok(())
}

pub fn run_remove(project: &Project, args: RemoveArgs) -> Result<()> {
    validate_var_name(&args.variable)?;
    let targets = select_remove_targets(project, &args)?;

    for (app, scope) in &targets {
        remove_from_app(app, &args.variable, scope)?;
    }

    let apps = targets.iter().map(|(app, _)| *app).collect::<Vec<_>>();
    print_mutation_result("removed", &args.variable, &apps);
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
        Fix::CreateDevVars { app } => format!("create {}/.dev.vars", display_rel(app)),
    }
}

fn apply_fixes(project: &Project, fixes: &[Fix]) -> Result<()> {
    let mut should_copy = false;
    for fix in fixes {
        match fix {
            Fix::BackfillExample { app, name } => {
                let workspace = app_workspaces(project)
                    .find(|workspace| workspace.rel == *app)
                    .ok_or_else(|| anyhow!("unknown app {}", app.display()))?;
                dotenv::upsert_example(&dotenv::example_path(workspace), name, "")?;
            }
            Fix::CreateLocalEnv | Fix::CreateDevVars { .. } => {
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
    }
    Ok(())
}

fn sync_local_env(project: &Project) -> Result<()> {
    let plan = build_copy_plan(project, true, false)?;
    apply_copy_plan(&plan)
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
) -> Result<Vec<(&'a crate::models::Workspace, RemoveScope)>> {
    if args.shared || args.owner.as_deref().is_some_and(is_shared_owner_alias) {
        let records = definition_records_for_variable(project, &args.variable)?;
        if records.is_empty() {
            bail!("{} was not found in any app owner", args.variable);
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

    let records = definition_records_for_variable(project, &args.variable)?;
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
            args.variable
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
