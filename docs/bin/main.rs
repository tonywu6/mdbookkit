use std::path::PathBuf;

use tracing::error_span;

use mdbookkit::{emit_error, error::ProgramExit, logging::init_logging};

mod postprocess;
mod preprocess;

fn main() {
    init_logging();
    let _span = error_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Postprocess { root_dir } => postprocess::run(root_dir),
        Command::Preprocess { command: None } => preprocess::run(),
        Command::Preprocess {
            command: Some(Preprocess::Supports { .. }),
        } => Ok(()),
    }
    .or_else(emit_error!())
    .exit()
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
