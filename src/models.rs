use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceKind {
    App,
    Package,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub rel: PathBuf,
    pub kind: WorkspaceKind,
    pub framework: String,
}

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub is_monorepo: bool,
    pub workspaces: Vec<Workspace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Scope {
    Private,
    Public,
    Unknown,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Private => "private",
            Scope::Public => "public",
            Scope::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub enum SourceKind {
    EnvExample,
    EnvLocal,
    TsSchema,
    PythonSchema,
    RustSchema,
}

impl SourceKind {
    pub fn surface(&self) -> EnvSurface {
        match self {
            SourceKind::TsSchema | SourceKind::PythonSchema | SourceKind::RustSchema => {
                EnvSurface::Schema
            }
            SourceKind::EnvExample => EnvSurface::Template,
            SourceKind::EnvLocal => EnvSurface::Local,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum EnvSurface {
    Schema,
    Template,
    Local,
    Sinks,
}

impl EnvSurface {
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvSurface::Schema => "schema",
            EnvSurface::Template => "template",
            EnvSurface::Local => "local",
            EnvSurface::Sinks => "sinks",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VarSource {
    pub name: String,
    pub owner: PathBuf,
    pub scope: Scope,
    pub kind: SourceKind,
    pub value_type: Option<String>,
    pub required: Option<bool>,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct EnvRecord {
    pub name: String,
    pub owner: PathBuf,
    pub scope: Scope,
    pub value_type: Option<String>,
    pub required: Option<bool>,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub example_value: Option<String>,
    pub local_present: bool,
    pub surfaces: BTreeSet<EnvSurface>,
    pub surface_sources: BTreeMap<EnvSurface, BTreeSet<String>>,
    pub sources: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct DotenvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
    pub fix: Option<Fix>,
}

#[derive(Clone, Debug)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fix {
    BackfillExample { app: PathBuf, name: String },
    CreateLocalEnv,
}

#[derive(Clone, Debug)]
pub struct VarMutation {
    pub variable: String,
    pub description: Option<String>,
    pub example: Option<String>,
    pub optional: bool,
    pub default_value: Option<String>,
    pub numeric: bool,
    pub number: bool,
    pub boolean: bool,
    pub enum_values: Option<String>,
    pub test_regex: Option<String>,
    pub test_regex_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CopyPlan {
    pub writes: Vec<FileWritePlan>,
}

#[derive(Clone, Debug)]
pub struct FileWritePlan {
    pub path: PathBuf,
    pub contents: String,
}
