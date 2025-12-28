use std::{env, fs, io::Write};

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let output = env::var("GITHUB_OUTPUT").context("missing $GITHUB_OUTPUT")?;
    let mut output = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output)
        .context("failed to open $GITHUB_OUTPUT")?;

    match clap::Parser::parse() {
        Command::WhichPackage { tag_name } => {
            for package in ["mdbook-rustdoc-links", "mdbook-permalinks"] {
                if (tag_name.as_ref()).is_some_and(|tag| tag.starts_with(package))
                    || tag_name.is_none()
                {
                    writeln!(&mut output, "{package}=true")?;
                }
            }
        }
    }

    Ok(())
}

#[derive(clap::Parser, Debug)]
enum Command {
    WhichPackage { tag_name: Option<String> },
}
