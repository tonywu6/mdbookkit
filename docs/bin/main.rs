use std::path::PathBuf;

use tracing::info_span;

use mdbookkit::{
    emit,
    error::{ConsumeError, ProgramExit},
    logging::Logging,
};

mod postprocess;

fn main() {
    Logging::default().init();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Postprocess { root_dir } => postprocess::run(root_dir),
    }
    .or_error(emit!())
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
