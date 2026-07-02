use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

use crate::adapters::dotenv;
use crate::discovery::app_workspaces;
use crate::graph::collect_sources;
use crate::models::{CopyPlan, DotenvEntry, FileWritePlan, Project, SourceKind};
use crate::ordering::{compare_env_names, env_group};
use crate::util::display_rel;

pub fn build_copy_plan(
    project: &Project,
    execute_templates: bool,
    overwrite: bool,
) -> Result<CopyPlan> {
    let env_path = project.root.join(".env");
    let existing_entries = dotenv::parse_file(&env_path)?;
    let existing = existing_entries
        .iter()
        .map(|entry| (entry.key.clone(), entry.value.clone()))
        .collect::<HashMap<_, _>>();

    let shared_names = shared_schema_names(project)?;
    let sections = example_sections(project)?;
    let documented_names = section_entry_names(&sections);
    let mut env_contents = render_env_contents(project, &sections, &shared_names, |entry| {
        let value = copy_entry_value(
            &entry.value,
            &entry.key,
            &existing,
            execute_templates,
            overwrite,
            &project.root,
        );
        format!("{}={}", entry.key, dotenv::quote_value(&value))
    });
    append_local_only_entries(&mut env_contents, &existing_entries, &documented_names);

    let mut writes = vec![FileWritePlan {
        path: env_path,
        contents: env_contents,
    }];

    if project.is_monorepo {
        writes.push(FileWritePlan {
            path: root_example_path(project),
            contents: render_env_contents(project, &sections, &shared_names, |entry| {
                format!("{}={}", entry.key, dotenv::quote_value(&entry.value))
            }),
        });
    }

    Ok(CopyPlan { writes })
}

pub fn build_root_example_plan(project: &Project) -> Result<Option<FileWritePlan>> {
    if !project.is_monorepo {
        return Ok(None);
    }

    let shared_names = shared_schema_names(project)?;
    let sections = example_sections(project)?;
    Ok(Some(FileWritePlan {
        path: root_example_path(project),
        contents: render_env_contents(project, &sections, &shared_names, |entry| {
            format!("{}={}", entry.key, dotenv::quote_value(&entry.value))
        }),
    }))
}

pub fn root_example_path(project: &Project) -> std::path::PathBuf {
    project.root.join(".env.example")
}

fn render_env_contents(
    project: &Project,
    sections: &[ExampleSection],
    shared_names: &BTreeSet<String>,
    mut render_entry: impl FnMut(&DotenvEntry) -> String,
) -> String {
    let mut output = Vec::new();

    if project.is_monorepo && !shared_names.is_empty() {
        let mut shared_entries = Vec::new();
        let mut seen_shared = BTreeSet::new();
        for section in sections {
            for entry in &section.entries {
                if !shared_names.contains(&entry.key) || !seen_shared.insert(entry.key.clone()) {
                    continue;
                }
                shared_entries.push(entry);
            }
        }
        let shared_lines = render_sorted_entries(shared_entries, &mut render_entry);

        if !shared_lines.is_empty() {
            output.push("# ---- shared ----".to_string());
            output.extend(shared_lines);
            output.push(String::new());
        }
    }

    let mut seen = BTreeSet::new();
    for section in sections {
        let mut section_entries = Vec::new();
        for entry in &section.entries {
            if shared_names.contains(&entry.key) || seen.contains(&entry.key) {
                continue;
            }
            seen.insert(entry.key.clone());
            section_entries.push(entry);
        }
        let section_lines = render_sorted_entries(section_entries, &mut render_entry);

        if section_lines.is_empty() {
            continue;
        }
        if project.is_monorepo {
            output.push(format!(
                "# ---- {}/.env.example ----",
                display_rel(&section.app_rel)
            ));
        }
        output.extend(section_lines);
        output.push(String::new());
    }

    format!("{}\n", output.join("\n").trim_end())
}

fn render_sorted_entries(
    mut entries: Vec<&DotenvEntry>,
    render_entry: &mut impl FnMut(&DotenvEntry) -> String,
) -> Vec<String> {
    entries.sort_by(|left, right| compare_env_names(&left.key, &right.key));

    let mut lines = Vec::new();
    let mut previous_group: Option<String> = None;
    for entry in entries {
        let group = env_group(&entry.key);
        if previous_group
            .as_ref()
            .is_some_and(|previous| previous != &group)
        {
            lines.push(String::new());
        }
        lines.push(render_entry(entry));
        previous_group = Some(group);
    }
    lines
}

fn section_entry_names(sections: &[ExampleSection]) -> BTreeSet<String> {
    sections
        .iter()
        .flat_map(|section| section.entries.iter().map(|entry| entry.key.clone()))
        .collect()
}

fn append_local_only_entries(
    contents: &mut String,
    existing_entries: &[DotenvEntry],
    documented_names: &BTreeSet<String>,
) {
    let local_only = existing_entries
        .iter()
        .filter(|entry| !documented_names.contains(&entry.key))
        .collect::<Vec<_>>();
    let lines = render_sorted_entries(local_only, &mut |entry| {
        format!("{}={}", entry.key, dotenv::quote_value(&entry.value))
    });

    if lines.is_empty() {
        return;
    }

    let mut output = contents.trim_end().to_string();
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str("# ---- local-only ----\n");
    output.push_str(&lines.join("\n"));
    output.push('\n');
    *contents = output;
}

fn shared_schema_names(project: &Project) -> Result<BTreeSet<String>> {
    if !project.is_monorepo {
        return Ok(BTreeSet::new());
    }

    let mut owners_by_name = BTreeMap::<String, BTreeSet<_>>::new();
    for source in collect_sources(project)? {
        if matches!(
            source.kind,
            SourceKind::TsSchema | SourceKind::PythonSchema | SourceKind::RustSchema
        ) {
            owners_by_name
                .entry(source.name)
                .or_default()
                .insert(source.owner);
        }
    }

    Ok(owners_by_name
        .into_iter()
        .filter_map(|(name, owners)| (owners.len() > 1).then_some(name))
        .collect())
}

fn example_sections(project: &Project) -> Result<Vec<ExampleSection>> {
    let mut sections = Vec::new();
    for app in app_workspaces(project) {
        let example = dotenv::example_path(app);
        if !example.exists() {
            continue;
        }
        let entries = dotenv::parse_file(&example)?;
        if entries.is_empty() {
            continue;
        }
        sections.push(ExampleSection {
            app_rel: app.rel.clone(),
            entries,
        });
    }
    Ok(sections)
}

fn copy_entry_value(
    example_value: &str,
    key: &str,
    existing: &HashMap<String, String>,
    execute_templates: bool,
    overwrite: bool,
    cwd: &Path,
) -> String {
    if overwrite {
        copy_value(example_value, execute_templates, cwd)
    } else {
        existing
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| copy_value(example_value, execute_templates, cwd))
    }
}

fn copy_value(value: &str, execute_templates: bool, cwd: &Path) -> String {
    if execute_templates {
        resolve_template_or_value(value, cwd).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

struct ExampleSection {
    app_rel: std::path::PathBuf,
    entries: Vec<DotenvEntry>,
}

fn resolve_template_or_value(value: &str, cwd: &Path) -> Result<String> {
    let Some(first_start) = value.find("$(") else {
        return Ok(value.to_string());
    };

    let mut output = String::new();
    let mut cursor = 0;
    let mut next_start = Some(first_start);

    while let Some(start) = next_start {
        output.push_str(&value[cursor..start]);
        let command_start = start + 2;
        let command_end = find_template_end(value, command_start)?;
        let command = &value[command_start..command_end];
        output.push_str(&run_template_command(command, cwd)?);
        cursor = command_end + 1;
        next_start = value[cursor..].find("$(").map(|index| cursor + index);
    }

    output.push_str(&value[cursor..]);
    Ok(output)
}

fn find_template_end(value: &str, command_start: usize) -> Result<usize> {
    let mut quote = None;
    let mut escaped = false;

    for (offset, ch) in value[command_start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if ch == ')' {
            return Ok(command_start + offset);
        }
    }

    bail!("unclosed template command");
}

fn run_template_command(command: &str, cwd: &Path) -> Result<String> {
    if command.contains('|')
        || command.contains(';')
        || command.contains('&')
        || command.contains('>')
        || command.contains('<')
    {
        bail!("shell template syntax is not enabled: {command}");
    }

    let argv = shlex::split(command).ok_or_else(|| anyhow!("invalid template command"))?;
    if argv.is_empty() {
        bail!("empty template command");
    }
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run template command `{command}`"))?;
    if !output.status.success() {
        bail!("template command failed: {command}");
    }

    let mut value = String::from_utf8(output.stdout)?;
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Workspace, WorkspaceKind};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn resolves_embedded_template_segments_from_project_root() {
        let tempdir = tempfile::tempdir().unwrap();
        let value = resolve_template_or_value("file:$(pwd)/local.db", tempdir.path()).unwrap();
        let expected_root = tempdir.path().canonicalize().unwrap();

        assert_eq!(value, format!("file:{}/local.db", expected_root.display()));
    }

    #[test]
    fn resolves_multiple_template_segments() {
        let tempdir = tempfile::tempdir().unwrap();
        let value =
            resolve_template_or_value("$(printf one)-$(printf two)", tempdir.path()).unwrap();

        assert_eq!(value, "one-two");
    }

    #[test]
    fn leaves_plain_values_unchanged() {
        let tempdir = tempfile::tempdir().unwrap();
        let value = resolve_template_or_value("file:./local.db", tempdir.path()).unwrap();

        assert_eq!(value, "file:./local.db");
    }

    #[test]
    fn puts_schema_shared_values_in_shared_section() {
        let tempdir = tempfile::tempdir().unwrap();
        write_app_fixture(
            tempdir.path(),
            "apps/api",
            "DATABASE_URL=file:./api.db\nAPI_KEY=api\n",
        );
        write_app_fixture(
            tempdir.path(),
            "apps/web",
            "DATABASE_URL=file:./web.db\nWEB_KEY=web\n",
        );

        let project = Project {
            root: tempdir.path().to_path_buf(),
            is_monorepo: true,
            workspaces: vec![
                test_workspace(tempdir.path(), "apps/api"),
                test_workspace(tempdir.path(), "apps/web"),
            ],
        };

        let plan = build_copy_plan(&project, false, true).unwrap();
        let env_contents = write_contents(&plan, ".env");
        let example_contents = write_contents(&plan, ".env.example");

        assert!(env_contents.starts_with("# ---- shared ----\nDATABASE_URL=file:./api.db"));
        assert!(env_contents.contains("\n# ---- apps/api/.env.example ----\nAPI_KEY=api"));
        assert!(env_contents.contains("\n# ---- apps/web/.env.example ----\nWEB_KEY=web"));
        assert!(!env_contents.contains("# ---- apps/api/.env.example ----\nDATABASE_URL"));
        assert!(!env_contents.contains("# ---- apps/web/.env.example ----\nDATABASE_URL"));

        assert_eq!(env_contents, example_contents);
    }

    #[test]
    fn monorepo_root_example_keeps_template_values() {
        let tempdir = tempfile::tempdir().unwrap();
        write_app_fixture(
            tempdir.path(),
            "apps/api",
            "DATABASE_URL=file:$(pwd)/api.db\nAPI_KEY=api\n",
        );

        let project = Project {
            root: tempdir.path().to_path_buf(),
            is_monorepo: true,
            workspaces: vec![test_workspace(tempdir.path(), "apps/api")],
        };

        let plan = build_copy_plan(&project, true, true).unwrap();
        let env_contents = write_contents(&plan, ".env");
        let example_contents = write_contents(&plan, ".env.example");

        assert!(env_contents.contains(&format!(
            "DATABASE_URL=file:{}/api.db",
            tempdir.path().canonicalize().unwrap().display()
        )));
        assert!(example_contents.contains("DATABASE_URL=\"file:$(pwd)/api.db\""));
        assert_eq!(plan.writes.len(), 2);
    }

    #[test]
    fn copy_preserves_local_only_entries() {
        let tempdir = tempfile::tempdir().unwrap();
        fs::write(
            tempdir.path().join(".env"),
            "DATABASE_URL=file:./local.db\nDEV_DISABLE_EMAILS=true\n",
        )
        .unwrap();
        fs::write(
            tempdir.path().join(".env.example"),
            "DATABASE_URL=file:./example.db\n",
        )
        .unwrap();

        let project = Project {
            root: tempdir.path().to_path_buf(),
            is_monorepo: false,
            workspaces: vec![test_workspace(tempdir.path(), ".")],
        };

        let plan = build_copy_plan(&project, false, false).unwrap();
        let env_contents = write_contents(&plan, ".env");

        assert!(env_contents.contains("DATABASE_URL=file:./local.db"));
        assert!(env_contents.contains("# ---- local-only ----\nDEV_DISABLE_EMAILS=true"));
    }

    #[test]
    fn overwrite_preserves_local_only_entries() {
        let tempdir = tempfile::tempdir().unwrap();
        fs::write(
            tempdir.path().join(".env"),
            "DATABASE_URL=file:./local.db\nDEV_DISABLE_EMAILS=true\n",
        )
        .unwrap();
        fs::write(
            tempdir.path().join(".env.example"),
            "DATABASE_URL=file:./example.db\n",
        )
        .unwrap();

        let project = Project {
            root: tempdir.path().to_path_buf(),
            is_monorepo: false,
            workspaces: vec![test_workspace(tempdir.path(), ".")],
        };

        let plan = build_copy_plan(&project, false, true).unwrap();
        let env_contents = write_contents(&plan, ".env");

        assert!(env_contents.contains("DATABASE_URL=file:./example.db"));
        assert!(env_contents.contains("# ---- local-only ----\nDEV_DISABLE_EMAILS=true"));
    }

    fn write_contents<'a>(plan: &'a CopyPlan, name: &str) -> &'a str {
        plan.writes
            .iter()
            .find(|write| write.path.file_name().unwrap() == name)
            .map(|write| write.contents.as_str())
            .unwrap()
    }

    fn write_app_fixture(root: &Path, rel: &str, env_example: &str) {
        let app = root.join(rel);
        let src = app.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(app.join(".env.example"), env_example).unwrap();
        fs::write(
            src.join("env.private.ts"),
            r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string(),
  },
});
"#,
        )
        .unwrap();
    }

    fn test_workspace(root: &Path, rel: &str) -> Workspace {
        Workspace {
            root: root.join(rel),
            rel: PathBuf::from(rel),
            kind: WorkspaceKind::App,
            framework: "test".to_string(),
        }
    }
}
