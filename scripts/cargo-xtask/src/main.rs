use std::process::exit;

use anyhow::Result;
use tap::TapFallible;
use tracing::error;

mod bin;
mod github;

fn main() -> Result<()> {
    init_logging();
    match clap::Parser::parse() {
        Program::GitHub { command } => command.run(),
        Program::Bin(command) => command.run(),
    }
    .tap_err(|err| {
        error!("{err:?}");
        exit(1)
    })
}

#[derive(clap::Parser, Debug)]
enum Program {
    #[clap(name = "github")]
    GitHub {
        #[clap(subcommand)]
        command: github::Command,
    },
    Bin(bin::Command),
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .without_time()
        .init();
}
