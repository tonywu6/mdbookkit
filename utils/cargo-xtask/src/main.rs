use anyhow::Result;

mod github;

fn main() -> Result<()> {
    match clap::Parser::parse() {
        Program::GitHub { command } => command.run(),
    }
}

#[derive(clap::Parser, Debug)]
enum Program {
    #[clap(name = "github")]
    GitHub {
        #[clap(subcommand)]
        command: github::Command,
    },
}
