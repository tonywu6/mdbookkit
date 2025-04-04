use std::{collections::HashMap, io::Write};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use mdbook::preprocess::PreprocessorContext;
use tap::{Pipe, TapFallible};

use mdbookkit::{
    bin::rustdoc_link::{
        cache::{Cache, FileCache},
        env::{Config, Environment},
        Client, Pages, Resolver,
    },
    diagnostics::Issue,
    env::{
        book_from_stdin, book_into_stdout, config_from_book, for_each_chapter_mut, iter_chapters,
        smart_punctuation, string_from_stdin,
    },
    log_warning,
    logging::{is_logging, ConsoleLogger},
};

#[tokio::main]
async fn main() -> Result<()> {
    ConsoleLogger::install("rustdoc-link");
    match Program::parse().command {
        Some(Command::Supports { .. }) => Ok(()),
        Some(Command::Markdown(options)) => markdown(options).await,
        None => mdbook().await,
    }
}

#[derive(Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    /// Link to Rust documentation à la rustdoc.
    ///
    /// Markdown is read from stdin and written to stdout.
    Markdown(Config),

    /// Supporting command for mdbook.
    ///
    /// See <https://rust-lang.github.io/mdBook/for_developers/preprocessors.html#hooking-into-mdbook>
    #[clap(hide = true)]
    Supports { renderer: String },
}

async fn mdbook() -> Result<()> {
    let (context, mut book) = book_from_stdin().context("failed to parse book content")?;

    let config = config(&context).context("failed to read preprocessor config from book.toml")?;

    let client = Environment::new(config)
        .context("failed to initialize `mdbook-rustdoc-link`")?
        .pipe(Client::new);

    let cached = FileCache::load(client.env()).await.ok();

    let mut content = Pages::default();

    for (path, ch) in iter_chapters(&book) {
        let stream = client.env().markdown(&ch.content).into_offset_iter();
        content
            .read(path.clone(), &ch.content, stream)
            .with_context(|| path.display().to_string())
            .context("failed to parse Markdown source:")?;
    }

    if let Some(cached) = cached {
        cached.resolve(&mut content).await.ok();
    }

    client
        .resolve(&mut content)
        .await
        .context("failed to resolve some links")?;

    let mut result = iter_chapters(&book)
        .filter_map(|(path, _)| {
            let output = content
                .emit(path, &client.env().emit_config())
                .tap_err(log_warning!())
                .ok()?;
            Some((path.clone(), output.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let env = client.stop().await;

    let status = content
        .reporter()
        .names(|path| path.display().to_string())
        .level(LevelFilter::Warn)
        .logging(is_logging())
        .colored(colors_enabled_stderr())
        .build()
        .to_stderr()
        .to_status();

    if content.modified() {
        FileCache::save(&env, &content).await.ok();
    }

    for_each_chapter_mut(&mut book, |path, ch| {
        if let Some(output) = result.remove(&path) {
            ch.content = output
        }
    });

    book_into_stdout(&book)?;

    env.config.fail_on_warnings.check(status.level())?;

    Ok(())
}

async fn markdown(config: Config) -> Result<()> {
    let client = Environment::new(config)
        .context("failed to initialize")?
        .pipe(Client::new);

    let source = string_from_stdin().context("failed to read Markdown source from stdin")?;

    let stream = client.env().markdown(&source).into_offset_iter();

    let mut content = Pages::one(&source, stream).context("failed to parse Markdown source")?;

    if let Ok(cached) = FileCache::load(client.env()).await {
        cached.resolve(&mut content).await.ok();
    }

    client
        .resolve(&mut content)
        .await
        .context("failed to resolve some links")?;

    let env = client.stop().await;

    let status = content
        .reporter()
        .names(|_| "<stdin>".into())
        .level(LevelFilter::Warn)
        .logging(is_logging())
        .colored(colors_enabled_stderr())
        .build()
        .to_stderr()
        .to_status();

    if content.modified() {
        FileCache::save(&env, &content).await.ok();
    }

    content
        .get(&env.emit_config())
        .map(|emit| emit.to_string())
        .and_then(|output| Ok(std::io::stdout().write_all(output.as_bytes())?))?;

    env.config.fail_on_warnings.check(status.level())?;

    Ok(())
}

fn config(context: &PreprocessorContext) -> Result<Config> {
    let mut config = config_from_book::<Config>(&context.config, "rustdoc-link")?;

    if let Some(path) = config.manifest_dir {
        config.manifest_dir = Some(context.root.join(path))
    } else {
        config.manifest_dir = Some(context.root.clone())
    }

    if let Some(path) = config.cache_dir {
        config.cache_dir = Some(context.root.join(path))
    }

    config.smart_punctuation = smart_punctuation(&context.config);

    Ok(config)
}
