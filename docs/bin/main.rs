use std::sync::LazyLock;

use camino::Utf8PathBuf;
use tracing::error_span;

use mdbookkit::{emit_error, env::locate_project, error::ProgramExit, logging::init_logging};

mod postprocess;
mod preprocess;
mod readme;

fn main() {
    init_logging();
    let _span = error_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Postprocess => postprocess::run().or_else(emit_error!()),
        Command::Preprocess { command: None } => preprocess::run(),
        Command::Preprocess {
            command: Some(Preprocess::Supports { .. }),
        } => Ok(()),
        Command::Readme => readme::render().or_else(emit_error!()),
    }
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
    Postprocess,
    Readme,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Preprocess {
    #[clap(hide = true)]
    Supports { renderer: String },
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Readme {
    #[clap(hide = true)]
    Supports {
        renderer: String,
    },
    Render,
}

static CARGO_WORKSPACE: LazyLock<Utf8PathBuf> =
    LazyLock::new(|| locate_project(None).or_else(emit_error!()).unwrap());
