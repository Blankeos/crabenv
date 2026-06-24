use anyhow::{anyhow, Context, Result};
use clap::Parser;

mod adapters;
mod cli;
mod commands;
mod copy_plan;
mod discovery;
mod graph;
mod issues;
mod models;
mod ordering;
mod prompt;
mod render;
mod sinks;
mod util;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    // Intercept Ctrl-C so cliclack prompts cancel gracefully (like ESC)
    // instead of killing the process with the cursor hidden.
    ctrlc::set_handler(|| {}).map_err(|e| anyhow!("setting Ctrl-C handler: {e}"))?;

    let cli = Cli::parse();
    let root = util::normalize_root(&cli.root)
        .with_context(|| format!("could not read root {}", cli.root.display()))?;
    let project = discovery::discover_project(&root)?;

    match cli.command {
        Commands::Init => commands::run_init(&project),
        Commands::List(args) => commands::run_list(&project, args),
        Commands::Doctor(args) => commands::run_doctor(&project, args),
        Commands::Format(args) => commands::run_format(&project, args),
        Commands::Copy(args) => commands::run_copy(&project, args),
        Commands::Add(args) => commands::run_add_or_update(&project, args, false),
        Commands::Attach(args) => commands::run_attach(&project, args),
        Commands::Update(args) => commands::run_add_or_update(&project, args, true),
        Commands::Remove(args) => commands::run_remove(&project, args),
    }
}
