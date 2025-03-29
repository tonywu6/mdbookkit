use std::{
    collections::HashMap,
    io::{Read, Write},
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use tap::{Pipe, TapFallible};

use mdbookkit::{
    bin::rustdoc_link::{
        cache::{Cache, FileCache},
        env::{Config, Environment},
        Client, Pages, Resolver,
    },
    diagnostics::Issue,
    env::{
        book_from_stdin, config_from_book, for_each_chapter_mut, iter_chapters, smart_punctuation,
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
    let (context, mut book) = book_from_stdin()?;

    let config = {
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

        config
    };

    let client = Client::new(Environment::new(config)?);

    let cached = FileCache::load(client.env()).await.ok();

    let mut content = Pages::default();

    for (path, ch) in iter_chapters(&book) {
        let stream = client.env().markdown(&ch.content).into_offset_iter();
        content.read(path.clone(), &ch.content, stream)?;
    }

    if let Some(cached) = cached {
        cached.resolve(&mut content).await.ok();
    }

    client.resolve(&mut content).await?;

    let mut result = iter_chapters(&book)
        .filter_map(|(path, _)| {
            content
                .emit(path, &client.env().emit_config())
                .tap_err(log_warning!())
                .ok()
                .map(|output| (path.clone(), output.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let env = client.stop().await?;

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

    let output = serde_json::to_string(&book)?;
    std::io::stdout().write_all(output.as_bytes())?;

    env.config.fail_on_unresolved.check(status.level())?;

    Ok(())
}

async fn markdown(options: Config) -> Result<()> {
    let client = Client::new(Environment::new(options)?);

    let source = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?;

    let stream = client.env().markdown(&source).into_offset_iter();

    let mut content = Pages::one(&source, stream)?;

    if let Ok(cached) = FileCache::load(client.env()).await {
        cached.resolve(&mut content).await.ok();
    }

    client.resolve(&mut content).await?;

    let env = client.stop().await?;

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

    let output = content.get(&env.emit_config())?.to_string();
    std::io::stdout().write_all(output.as_bytes())?;

    env.config.fail_on_unresolved.check(status.level())?;

    Ok(())
}
