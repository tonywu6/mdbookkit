use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use tap::TapFallible;

use mdbookkit::{
    bin::link_forever::{Environment, Pages},
    diagnostics::Issue,
    env::{book_from_stdin, book_into_stdout, for_each_chapter_mut, iter_chapters},
    log_warning,
    logging::{is_logging, ConsoleLogger},
};

fn main() -> Result<()> {
    ConsoleLogger::install("link-forever");

    match Program::parse().command {
        Some(Command::Supports { .. }) => return Ok(()),
        None => {}
    }

    let (context, mut book) = book_from_stdin().context("failed to parse book content")?;

    let env = match Environment::try_from_env(&context)
        .context("failed to initialize `mdbook-link-forever`")?
    {
        Ok(env) => env,
        Err(err) => {
            log::warn!("{:?}", err.context("preprocessor will be disabled"));
            return book_into_stdout(&book);
        }
    };

    let mut content = Pages::new(env.markdown);

    for (path, ch) in iter_chapters(&book) {
        let url = env
            .book_src
            .join(&path.to_string_lossy())
            .context("could not read path as a url")?;
        content
            .insert(url, &ch.content)
            .with_context(|| path.display().to_string())
            .context("failed to parse Markdown source:")?;
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

    book_into_stdout(&book)?;

    env.config.fail_on_warnings.check(status.level())?;

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
