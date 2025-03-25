use std::{
    collections::HashMap,
    io::{Read, Write},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::LevelFilter;
use mdbook::{book::Book, preprocess::PreprocessorContext, BookItem};
use serde::Deserialize;
use tap::{Pipe, TapFallible};

use mdbook_rustdoc_link::{
    cache::{Cache, FileCache},
    env::{Config, Environment},
    log_warning,
    logger::ConsoleLogger,
    preprocessor_name, Client, Pages, Resolver,
};

#[tokio::main]
async fn main() -> Result<()> {
    ConsoleLogger::install();
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
    Supports { renderer: String },
    Markdown(Config),
}

async fn mdbook() -> Result<()> {
    let (context, mut book): (PreprocessorContext, Book) = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(serde_json::from_str)?;

    let config = {
        let mut config = if let Some(config) = context.config.get_preprocessor(preprocessor_name())
        {
            Config::deserialize(toml::Value::Table(config.clone()))
                .context("failed to read preprocessor config from book.toml")?
        } else {
            Default::default()
        };

        if let Some(path) = config.manifest_dir {
            config.manifest_dir = Some(context.root.join(path))
        } else {
            config.manifest_dir = Some(context.root.clone())
        }

        if let Some(path) = config.cache_dir {
            config.cache_dir = Some(context.root.join(path))
        }

        config.smart_punctuation = context
            .config
            .get_deserialized_opt::<bool, _>("output.html.smart-punctuation")
            .unwrap_or_default()
            .unwrap_or(true);

        config
    };

    let client = Client::new(Environment::new(config)?);

    let cached = FileCache::load(client.env()).await.ok();

    let mut content = Pages::default();

    for item in book.iter() {
        let BookItem::Chapter(ch) = item else {
            continue;
        };
        let Some(key) = &ch.source_path else {
            continue;
        };
        let stream = client.env().markdown(&ch.content).into_offset_iter();
        content.read(key.clone(), &ch.content, stream)?;
    }

    if let Some(cached) = cached {
        cached.resolve(&mut content).await.ok();
    }

    client.resolve(&mut content).await?;

    let mut result = book
        .iter()
        .filter_map(|item| {
            let BookItem::Chapter(ch) = item else {
                return None;
            };
            let Some(key) = &ch.source_path else {
                return None;
            };
            content
                .emit(key, &client.env().emit_config())
                .tap_err(log_warning!())
                .ok()
                .map(|output| (key.clone(), output.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let env = client.stop().await?;

    let status = content
        .reporter()
        .paths(|path| path.display().to_string())
        .level(LevelFilter::Warn)
        .build()
        .to_stderr()
        .to_status();

    if content.modified() {
        FileCache::save(&env, &content).await.ok();
    }

    book.for_each_mut(|item| {
        let BookItem::Chapter(ch) = item else { return };
        let Some(key) = &ch.source_path else { return };
        if let Some(output) = result.remove(key) {
            ch.content = output
        }
    });

    let output = serde_json::to_string(&book)?;

    std::io::stdout().write_all(output.as_bytes())?;

    status.check(env.config.fail_on_unresolved)?;

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
        .paths(|_| "<stdin>".into())
        .level(LevelFilter::Warn)
        .build()
        .to_stderr()
        .to_status();

    if content.modified() {
        FileCache::save(&env, &content).await.ok();
    }

    let output = content.get(&env.emit_config())?.to_string();

    std::io::stdout().write_all(output.as_bytes())?;

    status.check(env.config.fail_on_unresolved)?;

    Ok(())
}
