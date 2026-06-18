use anyhow::Result;

use crate::models::{Project, VarSource, Workspace};

pub mod dotenv;
pub mod python;
pub mod typescript;

pub trait WorkspaceAdapter {
    fn name(&self) -> &'static str;
    fn collect(&self, project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>>;
}

pub trait ProjectAdapter {
    fn name(&self) -> &'static str;
    fn collect(&self, project: &Project) -> Result<Vec<VarSource>>;
}

struct DotenvExampleAdapter;
struct DotenvLocalAdapter;
struct DotenvRootLocalAdapter;
struct TypeScriptPrivateAdapter;
struct TypeScriptPublicAdapter;
struct PythonAdapter;

impl WorkspaceAdapter for DotenvExampleAdapter {
    fn name(&self) -> &'static str {
        "dotenv-example"
    }

    fn collect(&self, _project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
        dotenv::collect_example(workspace)
    }
}

impl WorkspaceAdapter for DotenvLocalAdapter {
    fn name(&self) -> &'static str {
        "dotenv-local"
    }

    fn collect(&self, project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
        dotenv::collect_local(project, workspace)
    }
}

impl ProjectAdapter for DotenvRootLocalAdapter {
    fn name(&self) -> &'static str {
        "dotenv-root-local"
    }

    fn collect(&self, project: &Project) -> Result<Vec<VarSource>> {
        dotenv::collect_root_local(project)
    }
}

impl WorkspaceAdapter for TypeScriptPrivateAdapter {
    fn name(&self) -> &'static str {
        "typescript-private"
    }

    fn collect(&self, _project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
        typescript::collect_private_schema(workspace)
    }
}

impl WorkspaceAdapter for TypeScriptPublicAdapter {
    fn name(&self) -> &'static str {
        "typescript-public"
    }

    fn collect(&self, _project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
        typescript::collect_public_schema(workspace)
    }
}

impl WorkspaceAdapter for PythonAdapter {
    fn name(&self) -> &'static str {
        "python"
    }

    fn collect(&self, _project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
        python::collect_schema(workspace)
    }
}

pub fn workspace_adapters() -> Vec<Box<dyn WorkspaceAdapter>> {
    vec![
        Box::new(DotenvExampleAdapter),
        Box::new(DotenvLocalAdapter),
        Box::new(TypeScriptPrivateAdapter),
        Box::new(TypeScriptPublicAdapter),
        Box::new(PythonAdapter),
    ]
}

pub fn project_adapters() -> Vec<Box<dyn ProjectAdapter>> {
    vec![Box::new(DotenvRootLocalAdapter)]
}

pub fn workspace_has_owned_env_files(workspace: &Workspace) -> bool {
    dotenv::example_path(workspace).exists()
        || typescript::private_schema_path(workspace).exists()
        || typescript::public_schema_path(workspace).exists()
        || typescript::should_use_plain_schema(workspace)
        || python::find_env_file(&workspace.root).is_some()
}
