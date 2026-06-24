use super::*;
use crate::models::{EnvRecord, Workspace, WorkspaceKind};
use std::collections::BTreeSet;

fn project(root: &Path) -> Project {
    Project {
        root: root.to_path_buf(),
        is_monorepo: true,
        workspaces: vec![Workspace {
            root: root.join("apps/web"),
            rel: PathBuf::from("apps/web"),
            kind: WorkspaceKind::App,
            framework: "typescript".to_string(),
        }],
    }
}

fn graph() -> EnvGraph {
    let mut graph = EnvGraph::new();
    insert_record(&mut graph, "NEXT_PUBLIC_GOOGLE_MAPS_API_KEY", Scope::Public);
    insert_record(&mut graph, "NEXT_PUBLIC_BASE_ORIGIN", Scope::Public);
    insert_record(&mut graph, "AUTH_SECRET", Scope::Private);
    graph
}

fn insert_record(graph: &mut EnvGraph, name: &str, scope: Scope) {
    let owner = PathBuf::from("apps/web");
    let mut surfaces = BTreeSet::new();
    surfaces.insert(EnvSurface::Schema);
    graph.insert(
        (owner.clone(), name.to_string()),
        EnvRecord {
            name: name.to_string(),
            owner,
            scope,
            value_type: None,
            enum_values: None,
            required: None,
            default_value: None,
            description: None,
            example_value: None,
            local_present: false,
            surfaces,
            surface_sources: BTreeMap::new(),
            sources: BTreeSet::new(),
        },
    );
}

#[test]
fn renders_public_gha_env_block_with_implicit_vars() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"jobs:
  build:
    env:
      # crabenv:start format=gha-env scope=public owner=apps/web
      OLD_VALUE: nope
      # crabenv:end
"#;

    let rendered = render_sink_file(&project, &graph(), &path, contents).unwrap();

    assert_eq!(rendered.changed_block_count, 1);
    assert_eq!(
        rendered.contents,
        r#"jobs:
  build:
    env:
      # crabenv:start format=gha-env scope=public owner=apps/web
      NEXT_PUBLIC_BASE_ORIGIN: ${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}
      NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
      # crabenv:end
"#
    );
}

#[test]
fn preserves_manual_gha_env_vars_or_secrets_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"env:
  # crabenv:start format=gha-env scope=public owner=apps/web
  NEXT_PUBLIC_BASE_ORIGIN: ${{ secrets.NEXT_PUBLIC_BASE_ORIGIN }}
  NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
  # crabenv:end
"#;

    let rendered = render_sink_file(&project, &graph(), &path, contents).unwrap();

    assert_eq!(rendered.changed_block_count, 0);
    assert_eq!(rendered.contents, contents);
}

#[test]
fn gha_env_accepts_all_scope_and_defaults_private_to_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"env:
  # crabenv:start format=gha-env scope=all owner=apps/web
  # crabenv:end
"#;

    let rendered = render_sink_file(&project, &graph(), &path, contents).unwrap();

    assert_eq!(
        rendered.contents,
        r#"env:
  # crabenv:start format=gha-env scope=all owner=apps/web
  AUTH_SECRET: ${{ secrets.AUTH_SECRET }}
  NEXT_PUBLIC_BASE_ORIGIN: ${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}
  NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
  # crabenv:end
"#
    );
}

#[test]
fn renders_gha_echo_block_to_dotenv_file() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"steps:
  - name: Create env file
    run: |
      # crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
      OLD_VALUE=nope
      # crabenv:end
"#;

    let rendered = render_sink_file(&project, &graph(), &path, contents).unwrap();

    assert_eq!(rendered.changed_block_count, 1);
    assert_eq!(
        rendered.contents,
        r#"steps:
  - name: Create env file
    run: |
      # crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
      echo "AUTH_SECRET=${{ secrets.AUTH_SECRET }}" >> .env.local
      echo "NEXT_PUBLIC_BASE_ORIGIN=${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}" >> .env.local
      echo "NEXT_PUBLIC_GOOGLE_MAPS_API_KEY=${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}" >> .env.local
      # crabenv:end
"#
    );
}

#[test]
fn preserves_manual_gha_echo_vars_or_secrets_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"run: |
  # crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
  echo "AUTH_SECRET=${{ vars.AUTH_SECRET }}" >> .env.local
  echo "NEXT_PUBLIC_BASE_ORIGIN=${{ secrets.NEXT_PUBLIC_BASE_ORIGIN }}" >> .env.local
  echo "NEXT_PUBLIC_GOOGLE_MAPS_API_KEY=${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}" >> .env.local
  # crabenv:end
"#;

    let rendered = render_sink_file(&project, &graph(), &path, contents).unwrap();

    assert_eq!(rendered.changed_block_count, 0);
    assert_eq!(rendered.contents, contents);
}

#[test]
fn rejects_missing_dest_for_gha_echo() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"run: |
  # crabenv:start format=gha-echo scope=all owner=apps/web
  # crabenv:end
"#;

    let error = render_sink_file(&project, &graph(), &path, contents).unwrap_err();

    assert!(error
        .to_string()
        .contains("missing required crabenv sink option `dest`"));
}

#[test]
fn rejects_source_option_for_gha_env() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let path = dir.path().join(".github/workflows/deploy.yml");
    let contents = r#"env:
  # crabenv:start format=gha-env scope=public owner=apps/web source=secrets
  # crabenv:end
"#;

    let error = render_sink_file(&project, &graph(), &path, contents).unwrap_err();

    assert!(error
        .to_string()
        .contains("unsupported crabenv sink option `source`"));
}
