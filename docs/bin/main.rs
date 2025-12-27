use std::{env::current_dir, path::PathBuf};

use anyhow::Result;

use tracing::info_span;

use mdbookkit::{emit_error, error::ExitProcess, logging::Logging};

mod postprocess;
mod preprocess;

fn main() -> Result<()> {
    Logging::default().init();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Postprocess { root_dir } => postprocess::run(root_dir.unwrap_or(current_dir()?)),
        Command::Preprocess { command: None } => preprocess::run(),
        Command::Preprocess {
            command: Some(Preprocess::Supports { .. }),
        } => Ok(()),
    }
    .exit(emit_error!());
    Ok(())
}

#[derive(clap::Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    Preprocess {
        #[command(subcommand)]
        command: Option<Preprocess>,
    },
    Postprocess {
        #[arg(long)]
        root_dir: Option<PathBuf>,
    },
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Preprocess {
    #[clap(hide = true)]
    Supports { renderer: String },
}
