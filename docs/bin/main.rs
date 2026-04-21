use std::path::PathBuf;

use tracing::error_span;

use mdbookkit::{emit_error, error::ProgramExit, logging::init_logging};

mod postprocess;

fn main() {
    init_logging();
    let _span = error_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Postprocess { root_dir } => postprocess::run(root_dir),
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
    Postprocess {
        #[arg(long)]
        root_dir: Option<PathBuf>,
    },
}
