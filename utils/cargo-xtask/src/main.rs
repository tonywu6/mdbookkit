use anyhow::Result;

mod github;
mod rust_analyzer;

fn main() -> Result<()> {
    match clap::Parser::parse() {
        Program::GitHub { command } => command.run(),
        Program::RustAnalyzer(command) => command.run(),
    }
}

#[derive(clap::Parser, Debug)]
enum Program {
    #[clap(name = "github")]
    GitHub {
        #[clap(subcommand)]
        command: github::Command,
    },
    RustAnalyzer(rust_analyzer::Program),
}
