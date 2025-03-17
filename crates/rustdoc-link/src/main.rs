use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
};

use anyhow::Result;

use mdbook::{book::Book, preprocess::PreprocessorContext, BookItem};
use mdbook_rustdoc_link::{
    cache::{Cache, Cacheable},
    env::Environment,
    item::Item,
    log_warning,
    logger::ConsoleLogger,
    markdown::{markdown_parser, Pages},
    preprocessor_name, Client, ClientConfig,
};
use serde::Deserialize;
use tap::{Pipe, TapFallible};

#[tokio::main]
async fn main() -> Result<()> {
    use clap::Parser;
    ConsoleLogger::init();
    match Command::parse().command {
        Some(Commands::Supports { .. }) => Ok(()),
        Some(Commands::Markdown(options)) => markdown(options).await,
        None => mdbook().await,
    }
}

#[derive(clap::Parser, Debug, Clone)]
struct Command {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Commands {
    Supports { renderer: String },
    Markdown(ClientConfig),
}

async fn mdbook() -> Result<()> {
    let (context, mut book): (PreprocessorContext, Book) = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(serde_json::from_str)?;

    let options = {
        let mut options = if let Some(config) = context.config.get_preprocessor(preprocessor_name())
        {
            ClientConfig::deserialize(toml::Value::Table(config.clone()))?
        } else {
            Default::default()
        };

        if let Some(path) = options.manifest_dir {
            options.manifest_dir = Some(context.root.join(path))
        } else {
            options.manifest_dir = Some(context.root.clone())
        }

        if let Some(path) = options.cache_dir {
            options.cache_dir = Some(context.root.join(path))
        }

        options.smart_punctuation = context
            .config
            .get_deserialized_opt::<bool, _>("output.html.smart-punctuation")
            .unwrap_or_default()
            .unwrap_or(true);

        options
    };

    let client = Client::new(Environment::new(options)?);

    let ClientConfig {
        prefer_local_links,
        smart_punctuation,
        ..
    } = client.env().config;

    let (pages, request) = book.iter().try_fold(
        (Pages::default(), HashSet::new()),
        |(mut pages, mut items), item| {
            let BookItem::Chapter(ch) = item else {
                return Ok((pages, items));
            };
            let Some(key) = &ch.source_path else {
                return Ok((pages, items));
            };
            let stream = markdown_parser(&ch.content, smart_punctuation).into_offset_iter();
            let parsed = match pages.read(key.clone(), &ch.content, stream) {
                Ok(parsed) => parsed,
                Err(error) => return Err(error),
            };
            items.extend(parsed);
            Ok((pages, items))
        },
    )?;

    let request = Item::parse_all(request.iter());

    let symbols = Client::request.cached::<Cache>(&client, request).await?;

    let mut result = book
        .iter()
        .filter_map(|item| {
            let BookItem::Chapter(ch) = item else {
                return None;
            };
            let Some(key) = &ch.source_path else {
                return None;
            };
            pages
                .emit(key, |k| symbols.get(k, prefer_local_links))
                .tap_err(log_warning!())
                .ok()
                .map(|output| (key.clone(), output))
        })
        .collect::<HashMap<_, _>>();

    book.for_each_mut(|item| {
        let BookItem::Chapter(ch) = item else { return };
        let Some(key) = &ch.source_path else { return };
        if let Some(output) = result.remove(key) {
            ch.content = output;
        }
    });

    client.dispose().await?;

    serde_json::to_string(&book)?.pipe(|out| std::io::stdout().write_all(out.as_bytes()))?;

    Ok(())
}

async fn markdown(options: ClientConfig) -> Result<()> {
    let client = Client::new(Environment::new(options)?);

    Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(|content| client.process(content))
        .await?
        .pipe(|output| std::io::stdout().write_all(output.as_bytes()))?;

    client.dispose().await?;

    Ok(())
}
