use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::models::VarMutation;

#[derive(Parser)]
#[command(name = "crabenv")]
#[command(
    about = "The simplest, opinionated way to keep .env files, schemas, and examples aligned."
)]
#[command(
    long_about = "crabenv is an opinionated, language-agnostic CLI that keeps your environment variables aligned across schema, template, and local files. No new config file required. It validates, copies, and checks for drift so your team doesn't have to."
)]
#[command(
    after_help = "Examples:\n  crabenv list\n  crabenv doctor\n  crabenv format\n  crabenv copy\n  crabenv add DATABASE_URL --owner apps/hono-api --example file:./local.db\n  crabenv add NEXT_PUBLIC_API_URL --owner apps/next-web --public --example http://localhost:8787\n  crabenv attach DATABASE_URL --from apps/hono-api --owner apps/next-web\n\nUse `crabenv <command> --help` for command-specific examples."
)]
pub struct Cli {
    #[arg(
        long,
        default_value = ".",
        help = "Project root to inspect or modify",
        long_help = "Project root to inspect or modify. In a monorepo this should usually be the repository root."
    )]
    pub root: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(
        about = "Initialize env files and schemas for discovered app owners",
        long_about = "Initialize missing env surfaces for discovered app owners. Existing files are never overwritten. Creates app .env.example files, schema files for TypeScript apps, language-specific schema files for Python/Rust apps, and local/root env files from templates."
    )]
    Init,
    #[command(
        visible_alias = "ls",
        about = "List env vars grouped by variable name",
        long_about = "List env vars from definition surfaces (schema and template). In an interactive terminal this opens a searchable list with focused details for the highlighted variable. Use -p/--print for the plain table, especially in scripts and AI agents. Managed sink coverage is shown as the sinks surface. Local-only values are hidden here and reported by doctor instead. Shared owner labels and enum values are expanded by default; use --compact for the shorter form."
    )]
    List(ListArgs),
    #[command(
        about = "Check env config surfaces for consistency",
        long_about = "Check schema/template drift, missing required local values, monorepo .env placement, public runtimeEnvStrict mappings, and managed sink drift. Also prints a full per-variable surfaces checklist including local-only variables."
    )]
    Doctor(DoctorArgs),
    #[command(
        visible_alias = "fmt",
        about = "Sort and group env files and schema entries",
        long_about = "Sort and group .env, .env.example, env.private.ts, and env.public.ts entries with crabenv's deterministic env ordering. Comments, section headers, placeholders, and raw values are preserved."
    )]
    Format(FormatArgs),
    #[command(
        visible_alias = "cp",
        about = "Create or update local env files from examples",
        long_about = "Create or update the local .env from discovered .env.example files. In monorepos this writes one root .env. Existing non-empty local values are preserved unless --overwrite is passed."
    )]
    Copy(CopyArgs),
    #[command(
        about = "Add an env var to one owner, selected owners, or every app owner with --shared",
        long_about = "Add an env var by updating the template (.env.example) and schema file. Use --owner in monorepos for one app. Use bare --shared or --shared '*' to write to every app owner; pass app owner paths to --shared to write to selected owners. Private scope is the default; pass --public for env.public.ts.",
        after_help = "Examples:\n  crabenv add DATABASE_URL --example file:./local.db\n  crabenv add DATABASE_URL --owner apps/hono-api --example file:./local.db\n  crabenv add DATABASE_URL --shared --example file:./local.db\n  crabenv add DATABASE_URL --shared apps/hono-api apps/next-web --example file:./local.db\n  crabenv add DATABASE_URL --shared=apps/hono-api,apps/next-web --example file:./local.db\n  crabenv add HONO_PORT --owner apps/hono-api --number --optional --example 8787\n  crabenv add LOG_LEVEL --owner apps/api --enum debug,info,warn,error --default info\n  crabenv add NEXT_PUBLIC_API_URL --owner apps/next-web --public --example http://localhost:8787"
    )]
    Add(MutateArgs),
    #[command(
        about = "Attach an existing env var to another owner",
        long_about = "Copy an existing env var contract from one owner into another owner. This is how a variable becomes shared: crabenv does not store shared metadata, it derives shared(N) when multiple owners define the same variable.",
        after_help = "Examples:\n  crabenv attach DATABASE_URL --from apps/hono-api --owner apps/next-web\n  crabenv attach REDIS_URL --owner apps/worker\n\nIf the variable exists in multiple owners, pass --from so crabenv knows which contract to copy."
    )]
    Attach(AttachArgs),
    #[command(
        about = "Update selected fields on an existing env var",
        long_about = "Update an existing env var by changing only the fields you pass. For example, --example only updates .env.example; --description only updates the schema comment while preserving the current type/default. Pass --owner in monorepos, or use bare --shared / --shared '*' for every app owner.",
        after_help = "Examples:\n  crabenv update DATABASE_URL --owner apps/hono-api --example file:./new-local.db\n  crabenv update DATABASE_URL --shared --example file:./new-local.db\n  crabenv update DATABASE_URL --shared apps/hono-api apps/next-web --example file:./new-local.db\n  crabenv update LOG_LEVEL --owner apps/api --enum debug,info,warn,error --default debug\n  crabenv update SENTRY_DSN --optional=false"
    )]
    Update(MutateArgs),
    #[command(
        about = "Remove an env var from one owner, selected owners, or every owner with --shared",
        long_about = "Remove an env var by deleting it from template (.env.example) and schema files. In monorepos, crabenv can infer the owner if only one app defines the variable. Use bare --shared or --shared '*' to remove from every owner that defines it; pass owner paths to remove from selected owners.",
        after_help = "Examples:\n  crabenv remove DATABASE_URL --owner apps/next-web\n  crabenv remove DATABASE_URL --shared\n  crabenv remove DATABASE_URL --shared apps/hono-api apps/next-web\n  crabenv remove NEXT_PUBLIC_API_URL --owner apps/next-web --public"
    )]
    Remove(RemoveArgs),
}

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(long, help = "Print and apply available automatic fixes")]
    pub fix: bool,
    #[arg(long, help = "Apply fixes instead of only printing the fix plan")]
    pub yes: bool,
}

#[derive(Args, Clone)]
pub struct ListArgs {
    #[arg(
        short = 'p',
        long,
        help = "Print the plain table instead of opening the interactive list"
    )]
    pub print: bool,

    #[arg(long, help = "Print compact JSON output; only valid with -p/--print")]
    pub json: bool,

    #[arg(short = 'x', long, hide = true)]
    pub expand: bool,

    #[arg(long, help = "Use compact shared owner labels and enum types")]
    pub compact: bool,
}

#[derive(Args, Clone)]
pub struct FormatArgs {
    #[arg(long, help = "Print files that would change without writing them")]
    pub check: bool,
}

#[derive(Args, Clone)]
pub struct CopyArgs {
    #[arg(long, help = "Print files that would be written without changing them")]
    pub dry_run: bool,
    #[arg(long, help = "Replace existing local values with .env.example values")]
    pub overwrite: bool,
    #[arg(
        long,
        help = "Do not execute template values like $(openssl rand -base64 32)"
    )]
    pub no_templates: bool,
}

#[derive(Args, Clone)]
pub struct MutateArgs {
    #[arg(
        index = 1,
        help = "Environment variable name, for example DATABASE_URL"
    )]
    pub variable: Option<String>,

    #[arg(
        long,
        value_name = "OWNER",
        help = "App owner path, for example apps/next-web. Use --shared for multiple app owners"
    )]
    pub owner: Option<PathBuf>,

    #[arg(
        long,
        value_name = "OWNER",
        num_args = 0..,
        value_delimiter = ',',
        action = ArgAction::Append,
        help = "Apply to every app owner (bare or '*') or selected app owners"
    )]
    pub shared: Option<Vec<PathBuf>>,

    #[arg(long, help = "Write to env.public.ts instead of env.private.ts")]
    pub public: bool,

    #[arg(
        long,
        value_name = "VALUE",
        help = "Safe value to write to .env.example"
    )]
    pub example: Option<String>,

    #[arg(
        long,
        value_name = "TEXT",
        help = "Description to write as a schema comment"
    )]
    pub description: Option<String>,

    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "true",
        help = "Mark the schema value as optional; pass --optional=false to mark required"
    )]
    pub optional: Option<bool>,

    #[arg(
        long = "default",
        value_name = "VALUE",
        help = "Set a schema default value"
    )]
    pub default_value: Option<String>,

    #[arg(long, help = "Use z.string()")]
    pub string: bool,

    #[arg(long, help = "Use a numeric string regex")]
    pub numeric: bool,

    #[arg(long, help = "Use z.coerce.number()")]
    pub number: bool,

    #[arg(long, help = "Use z.coerce.boolean()")]
    pub boolean: bool,

    #[arg(
        long = "enum",
        value_name = "CSV",
        help = "Use z.enum([...]) from comma-separated values"
    )]
    pub enum_values: Option<String>,

    #[arg(
        long = "testRegex",
        value_name = "REGEX",
        help = "Use z.string().regex(/REGEX/)"
    )]
    pub test_regex: Option<String>,

    #[arg(
        long = "testRegexMessage",
        value_name = "MESSAGE",
        help = "Message for --testRegex validation failures"
    )]
    pub test_regex_message: Option<String>,
}

impl From<&MutateArgs> for VarMutation {
    fn from(args: &MutateArgs) -> Self {
        Self {
            variable: args.variable.clone().unwrap_or_default(),
            description: args.description.clone(),
            example: args.example.clone(),
            optional: args.optional.unwrap_or(false),
            default_value: args.default_value.clone(),
            numeric: args.numeric,
            number: args.number,
            boolean: args.boolean,
            enum_values: args.enum_values.clone(),
            test_regex: args.test_regex.clone(),
            test_regex_message: args.test_regex_message.clone(),
        }
    }
}

#[derive(Args, Clone)]
pub struct AttachArgs {
    #[arg(help = "Existing environment variable name to attach")]
    pub variable: Option<String>,

    #[arg(
        long,
        value_name = "OWNER",
        help = "Source owner to copy the existing contract from"
    )]
    pub from: Option<PathBuf>,

    #[arg(
        long,
        value_name = "OWNER",
        help = "Target owner to add the existing contract to"
    )]
    pub owner: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct RemoveArgs {
    #[arg(index = 1, help = "Environment variable name to remove")]
    pub variable: Option<String>,

    #[arg(
        long,
        value_name = "OWNER",
        help = "App owner path, for example apps/next-web. Use --shared for multiple defining owners"
    )]
    pub owner: Option<PathBuf>,

    #[arg(
        long,
        value_name = "OWNER",
        num_args = 0..,
        value_delimiter = ',',
        action = ArgAction::Append,
        help = "Remove from every defining owner (bare or '*') or selected defining owners"
    )]
    pub shared: Option<Vec<PathBuf>>,

    #[arg(long, help = "Remove from env.public.ts instead of env.private.ts")]
    pub public: bool,
}
