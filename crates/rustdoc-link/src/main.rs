use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use lsp_types::Position;
use mdbook::{book::Book, preprocess::PreprocessorContext, BookItem};
use serde::Deserialize;
use tap::{Pipe, TapFallible};
use tokio::task::JoinSet;

use crate::{
    cache::{Cache, Cacheable},
    client::{Client, ItemLinks},
    env::Environment,
    item::{Carets, Item},
    markdown::{markdown_parser, Pages},
    terminal::{spinner, TermLogger},
};

mod cache;
mod client;
mod env;
mod error;
mod item;
mod markdown;
mod sync;
mod terminal;

#[derive(clap::Parser, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
struct BuildOptions {
    #[arg(long)]
    #[serde(default)]
    manifest_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    cache_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    pub smart_punctuation: bool,

    #[arg(long)]
    #[serde(default)]
    pub prefer_local_links: bool,
}

#[derive(clap::Parser, Debug, Clone)]
struct Command {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Commands {
    Supports { renderer: String },
    Markdown(BuildOptions),
}

#[tokio::main]
async fn main() -> Result<()> {
    use clap::Parser;
    TermLogger::init();
    match Command::parse().command {
        Some(Commands::Supports { .. }) => Ok(()),
        Some(Commands::Markdown(options)) => markdown(options).await,
        None => mdbook().await,
    }
}

async fn mdbook() -> Result<()> {
    let (context, mut book): (PreprocessorContext, Book) = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(serde_json::from_str)?;

    let options = {
        let mut options = if let Some(config) = context.config.get_preprocessor(preprocessor_name())
        {
            BuildOptions::deserialize(toml::Value::Table(config.clone()))?
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

    let config = Environment::new(options)?;

    let client = Client::new(config);

    let (pages, request) = book.iter().fold(
        (Pages::default(), HashSet::new()),
        |(mut pages, mut items), item| {
            let BookItem::Chapter(ch) = item else {
                return (pages, items);
            };
            let Some(key) = &ch.source_path else {
                return (pages, items);
            };
            let stream = markdown_parser(&ch.content, client.env.build_opts.smart_punctuation);
            items.extend(pages.read(key.clone(), stream));
            (pages, items)
        },
    );

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
            let BuildOptions {
                prefer_local_links, ..
            } = client.env.build_opts;
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

async fn markdown(options: BuildOptions) -> Result<()> {
    let config = Environment::new(options)?;

    let client = Client::new(config);

    let mut pages = Pages::default();

    let stream = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?;

    let stream = markdown_parser(&stream, client.env.build_opts.smart_punctuation);

    let request = Item::parse_all(pages.read((), stream).iter());

    let symbols = Client::request.cached::<Cache>(&client, request).await?;

    let BuildOptions {
        prefer_local_links, ..
    } = client.env.build_opts;

    let output = pages.emit(&(), |k| symbols.get(k, prefer_local_links))?;

    std::io::stdout().write_all(output.as_bytes())?;

    client.dispose().await?;

    Ok(())
}

impl Client {
    async fn request(&self, request: Vec<Item>) -> Result<SymbolMap> {
        let src = std::fs::read_to_string(self.env.entrypoint.path())?;

        let request = Request::new(&src, request);

        let mut items = HashMap::new();

        if request.request.is_empty() {
            return Ok(SymbolMap { items });
        }

        let document = self
            .open(self.env.entrypoint.clone(), request.context)
            .await?
            .pipe(Arc::new);

        spinner().create("resolve", Some(request.request.len() as _));

        let mut tasks: JoinSet<Result<(Item, ItemLinks)>> = JoinSet::new();

        for (item, line) in request.request {
            let document = document.clone();
            tasks.spawn(async move {
                let positions = match item.cols {
                    Carets::Decl(c1, c2) => &[
                        Position::new(line as _, c1 as _),
                        Position::new(line as _, c2 as _),
                    ] as &[Position],
                    Carets::Expr(c) => &[Position::new(line as _, c as _)],
                };

                spinner().task("resolve", &item.key);

                for &p in positions {
                    let links = document
                        .resolve(p)
                        .await
                        .context("error while resolving external docs")
                        .tap_err(log_debug!())
                        .unwrap_or_default();

                    if !links.is_empty() {
                        spinner().done("resolve", &item.key);
                        let links = links.with_fragment(item.fragment.as_deref());
                        return Ok((item, links));
                    }
                }

                spinner().done("resolve", &item.key);
                Ok((item, Default::default()))
            });
        }

        while let Some(res) = tasks.join_next().await {
            if let Ok(Ok((item, links))) = res {
                items.insert(item.key, links);
            };
        }

        spinner().finish("resolve", "done");

        Ok(SymbolMap { items })
    }
}

#[derive(Debug)]
struct Request {
    context: String,
    request: Vec<(Item, Line)>,
}

type Line = usize;

impl Request {
    fn new(source: &str, request: Vec<Item>) -> Self {
        let mut context = format!("{source}\nfn __6c0db446e2fa428eb93e3c71945e9654() {{\n");

        let mut current = context.chars().filter(|&c| c == '\n').count();

        let request = request
            .into_iter()
            .map(|item| {
                use std::fmt::Write;
                let _ = writeln!(context, "{}", item.stmt);
                let line = current;
                current += 1;
                (item, line)
            })
            .collect();

        context.push('}');

        Self { context, request }
    }
}

#[derive(Debug)]
struct SymbolMap {
    items: HashMap<String, ItemLinks>,
}

impl SymbolMap {
    fn get(&self, key: &str, local: bool) -> Option<&str> {
        let sym = self.items.get(key)?;
        if local {
            sym.local.as_ref()
        } else {
            sym.web.as_ref()
        }
        .map(|u| u.as_str())
    }
}

fn preprocessor_name() -> &'static str {
    let name = env!("CARGO_PKG_NAME");
    if let Some(idx) = name.find('-') {
        &name[idx + 1..]
    } else {
        name
    }
}
