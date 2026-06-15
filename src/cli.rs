use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::models::VarMutation;

#[derive(Parser)]
#[command(name = "crabenv")]
#[command(about = "Adapter-first environment variable management")]
#[command(
    long_about = "crabenv manages environment variables across app owners without introducing a new config file.\n\nEnv config is synced across surfaces: schema files such as src/env.private.ts and src/env.public.ts, template files such as .env.example, the local .env, and sink files such as Docker or Wrangler. In monorepos, app owners live at paths like apps/web and shared variables are derived when the same variable exists in multiple owners."
)]
#[command(
    after_help = "Examples:\n  crabenv list\n  crabenv doctor\n  crabenv copy\n  crabenv add DATABASE_URL --owner apps/hono-api --example file:./local.db\n  crabenv add NEXT_PUBLIC_API_URL --owner apps/next-web --public --example http://localhost:8787\n  crabenv attach DATABASE_URL --from apps/hono-api --owner apps/next-web\n\nUse `crabenv <command> --help` for command-specific examples."
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
        about = "Show discovered env owners and expected files",
        long_about = "Show the project mode, app owners, and whether each app has expected schema and template surfaces. With --fix, also runs the doctor fix plan."
    )]
    Init(InitArgs),
    #[command(
        visible_alias = "ls",
        about = "List env vars grouped by variable name",
        long_about = "List env vars from definition surfaces (schema and template), grouped by variable name with owner, scope, type, and surfaces. Local-only values are hidden here and reported by doctor instead. If the same schema-owned variable appears in multiple app owners, crabenv shows it as shared(N)."
    )]
    List,
    #[command(
        about = "Check env config surfaces for consistency",
        long_about = "Check schema/template drift, missing required local values, monorepo .env placement, public runtimeEnvStrict mappings, and sink references such as Docker or Wrangler. Also prints a full per-variable surfaces checklist including local-only variables."
    )]
    Doctor(DoctorArgs),
    #[command(
        visible_alias = "cp",
        about = "Create or update local env files from examples",
        long_about = "Create or update the local .env from discovered .env.example files. In monorepos this writes one root .env. For Cloudflare apps, it also writes .dev.vars. Existing non-empty local values are preserved unless --overwrite is passed."
    )]
    Copy(CopyArgs),
    #[command(
        about = "Add an env var to one owner, or every app owner with --shared",
        long_about = "Add an env var by updating the template (.env.example) and schema file. Use --owner in monorepos for one app. Use --shared to write the same variable to every app owner so it is derived as shared(N). Private scope is the default; pass --public for env.public.ts.",
        after_help = "Examples:\n  crabenv add DATABASE_URL --example file:./local.db\n  crabenv add DATABASE_URL --owner apps/hono-api --example file:./local.db\n  crabenv add DATABASE_URL --shared --example file:./local.db\n  crabenv add HONO_PORT --owner apps/hono-api --number --optional --example 8787\n  crabenv add LOG_LEVEL --owner apps/api --enum debug,info,warn,error --default info\n  crabenv add NEXT_PUBLIC_API_URL --owner apps/next-web --public --example http://localhost:8787"
    )]
    Add(MutateArgs),
    #[command(
        about = "Attach an existing env var to another owner",
        long_about = "Copy an existing env var contract from one owner into another owner. This is how a variable becomes shared: crabenv does not store shared metadata, it derives shared(N) when multiple owners define the same variable.",
        after_help = "Examples:\n  crabenv attach DATABASE_URL --from apps/hono-api --owner apps/next-web\n  crabenv attach REDIS_URL --owner apps/worker\n\nIf the variable exists in multiple owners, pass --from so crabenv knows which contract to copy."
    )]
    Attach(AttachArgs),
    #[command(
        about = "Replace an env var definition for one owner, or every app owner with --shared",
        long_about = "Replace an env var definition by updating the template (.env.example) and schema entry. This has the same flags as add, but communicates intent when changing an existing variable. Use --shared to update every app owner.",
        after_help = "Examples:\n  crabenv update DATABASE_URL --owner apps/hono-api --example file:./new-local.db\n  crabenv update DATABASE_URL --shared --example file:./new-local.db\n  crabenv update LOG_LEVEL --owner apps/api --enum debug,info,warn,error --default debug"
    )]
    Update(MutateArgs),
    #[command(
        about = "Remove an env var from one owner, or every owner with --shared",
        long_about = "Remove an env var by deleting it from template (.env.example) and schema files. In monorepos, crabenv can infer the owner if only one app defines the variable. Use --shared to remove the variable from every owner that defines it.",
        after_help = "Examples:\n  crabenv remove DATABASE_URL --owner apps/next-web\n  crabenv remove DATABASE_URL --shared\n  crabenv remove NEXT_PUBLIC_API_URL --owner apps/next-web --public"
    )]
    Remove(RemoveArgs),
}

#[derive(Args)]
pub struct InitArgs {
    #[arg(long, help = "Also run doctor fixes after showing discovered files")]
    pub fix: bool,
    #[arg(
        long,
        help = "Apply fix plans without asking for another confirmation flag"
    )]
    pub yes: bool,
}

#[derive(Args)]
pub struct DoctorArgs {
    #[arg(long, help = "Print and apply available automatic fixes")]
    pub fix: bool,
    #[arg(long, help = "Apply fixes instead of only printing the fix plan")]
    pub yes: bool,
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
    #[arg(help = "Environment variable name, for example DATABASE_URL")]
    pub variable: String,

    #[arg(
        long,
        value_name = "OWNER",
        help = "App owner path, for example apps/next-web. Use --shared for every app owner"
    )]
    pub owner: Option<PathBuf>,

    #[arg(
        long,
        help = "Apply to every app owner so the variable is derived as shared"
    )]
    pub shared: bool,

    #[arg(long, help = "Write to env.public.ts instead of env.private.ts")]
    pub public: bool,

    #[arg(
        long,
        value_name = "VALUE",
        help = "Safe value to write to .env.example"
    )]
    pub example: Option<String>,

    #[arg(long, help = "Mark the schema value as optional")]
    pub optional: bool,

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
            variable: args.variable.clone(),
            example: args.example.clone(),
            optional: args.optional,
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
    pub variable: String,

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
    pub owner: PathBuf,
}

#[derive(Args, Clone)]
pub struct RemoveArgs {
    #[arg(help = "Environment variable name to remove")]
    pub variable: String,

    #[arg(
        long,
        value_name = "OWNER",
        help = "App owner path, for example apps/next-web. Use --shared to remove from every defining owner"
    )]
    pub owner: Option<PathBuf>,

    #[arg(long, help = "Remove from every owner that defines this variable")]
    pub shared: bool,

    #[arg(long, help = "Remove from env.public.ts instead of env.private.ts")]
    pub public: bool,
}
