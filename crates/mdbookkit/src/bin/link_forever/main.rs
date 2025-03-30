use std::{collections::HashMap, io::Write};

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use mdbookkit::{
    bin::link_forever::{Environment, Pages},
    diagnostics::Issue,
    env::{book_from_stdin, for_each_chapter_mut, iter_chapters},
    log_warning,
    logging::{is_logging, ConsoleLogger},
};
use tap::TapFallible;

fn main() -> Result<()> {
    ConsoleLogger::install("link-forever");

    match Program::parse().command {
        Some(Command::Supports { .. }) => return Ok(()),
        None => {}
    }

    let (context, mut book) = book_from_stdin()?;

    let env = Environment::from_book(&context)?;

    let mut content = Pages::new(env.markdown);

    for (path, ch) in iter_chapters(&book) {
        let url = env.book_src.join(&path.to_string_lossy())?;
        content.insert(url, &ch.content)?;
    }

    env.resolve(&mut content);

    let mut result = iter_chapters(&book)
        .filter_map(|(path, _)| {
            let url = env.book_src.join(&path.to_string_lossy()).unwrap();
            content
                .emit(&url)
                .tap_err(log_warning!())
                .ok()
                .map(|output| (path.clone(), output.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let status = env
        .report(&content)
        .names(|url| env.rel_path(url))
        .level(LevelFilter::Warn)
        .logging(is_logging())
        .colored(colors_enabled_stderr())
        .build()
        .to_stderr()
        .to_status();

    for_each_chapter_mut(&mut book, |path, ch| {
        if let Some(output) = result.remove(&path) {
            ch.content = output
        }
    });

    let output = serde_json::to_string(&book)?;
    std::io::stdout().write_all(output.as_bytes())?;

    env.config.fail_on_unresolved.check(status.level())?;

    Ok(())
}

#[derive(Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    #[clap(hide = true)]
    Supports { renderer: String },
}
