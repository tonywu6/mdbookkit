use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::Result;
use lsp_types::Position;
use mdbook::{book::Book, preprocess::PreprocessorContext, BookItem};
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible, TapOptional};
use tokio::task::JoinSet;

use crate::{
    client::{document_position, Client, ExternalDocLinks, ExternalDocs},
    item::ItemName,
    markdown::{markdown_parser, Pages},
};

mod client;
mod item;
mod markdown;
mod sync;

#[derive(clap::Parser, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
struct BuildOptions {
    #[arg(long)]
    #[serde(default)]
    manifest_dir: Option<PathBuf>,
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
    env_logger::init();
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
            options.manifest_dir = Some(context.root)
        }
        options.smart_punctuation = context
            .config
            .get_deserialized_opt::<bool, _>("output.html.smart-punctuation")
            .unwrap_or_default()
            .unwrap_or(true);
        options
    };

    let (mut client, dispose) = Client::spawn(options).await?;

    let (pages, items) = book.iter().fold(
        (Pages::default(), HashSet::new()),
        |(mut pages, mut items), item| {
            let BookItem::Chapter(ch) = item else {
                return (pages, items);
            };
            let Some(key) = &ch.source_path else {
                return (pages, items);
            };
            let stream = markdown_parser(&ch.content, client.config.build_opts.smart_punctuation);
            items.extend(pages.read(key.clone(), stream));
            (pages, items)
        },
    );

    let links = client.resolve(items.into_iter().collect()).await?;

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
                .emit(key, |k| links.get(k))
                .tap_err(|e| log::warn!("{e:?}"))
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

    dispose.of(client).await?;

    serde_json::to_string(&book)?.pipe(|out| std::io::stdout().write_all(out.as_bytes()))?;

    Ok(())
}

async fn markdown(options: BuildOptions) -> Result<()> {
    let (mut client, dispose) = Client::spawn(options).await?;

    let mut pages = Pages::default();

    let stream = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?;

    let stream = markdown_parser(&stream, client.config.build_opts.smart_punctuation);

    let items = pages.read((), stream);

    let links = client.resolve(items.into_iter().collect()).await?;

    let output = pages.emit(&(), |k| links.get(k))?;

    std::io::stdout().write_all(output.as_bytes())?;

    dispose.of(client).await?;

    Ok(())
}

impl Client {
    async fn resolve(&mut self, request: Vec<String>) -> Result<ItemLinks> {
        let src = std::fs::read_to_string(self.config.entrypoint.path())?;

        let request = ItemRequestBatch::new(&src, request);

        let mut links = HashMap::new();

        let local = self.config.build_opts.prefer_local_links;

        if request.request.is_empty() {
            return Ok(ItemLinks { links, local });
        }

        let _document = self
            .open(self.config.entrypoint.clone(), request.context)
            .await?;

        let mut tasks = JoinSet::new();

        for ItemRequest {
            path,
            hash,
            position,
        } in &request.request
        {
            if links.contains_key(path) {
                continue;
            }

            let server = self.server.clone();
            let uri = self.config.entrypoint.clone();
            let pos = *position;
            let path = path.clone();
            let hash = hash.clone();

            tasks.spawn(async move {
                let ExternalDocLinks { web, local } = server
                    .request::<ExternalDocs>(document_position(uri, pos))
                    .await
                    .tap_err(|err| log::warn!("{err:#?}"))
                    .unwrap_or_default()?;

                let (web, local) = if let Some(hash) = hash.as_deref() {
                    let web = web.tap_some_mut(|u| u.set_fragment(Some(hash)));
                    let local = local.tap_some_mut(|u| u.set_fragment(Some(hash)));
                    (web, local)
                } else {
                    (web, local)
                };

                if web.is_none() && local.is_none() {
                    None
                } else {
                    let links = ExternalDocLinks { web, local };
                    let key = if let Some(hash) = hash {
                        format!("{path}#{hash}")
                    } else {
                        path
                    };
                    Some((key, links))
                }
            });
        }

        while let Some(res) = tasks.join_next().await {
            if let Ok(Some((key, resolved))) = res {
                links.insert(key, resolved);
            };
        }

        Ok(ItemLinks { links, local })
    }
}

#[derive(Debug)]
struct ItemRequestBatch {
    context: String,
    request: Vec<ItemRequest>,
}

#[derive(Debug)]
struct ItemRequest {
    path: String,
    hash: Option<String>,
    position: Position,
}

impl ItemRequestBatch {
    fn new(source: &str, items: Vec<String>) -> Self {
        use syn::parse::{Parse, Parser};

        let source = format!("{source}\nfn __6c0db446e2fa428eb93e3c71945e9654() {{\n");

        let mut request = vec![];
        let mut line = source.chars().filter(|&c| c == '\n').count();

        let context = HashSet::<String>::from_iter(items)
            .into_iter()
            .filter_map(|name| {
                let mut name = name.split('#');
                let path = name.next().unwrap();
                let item = ItemName::parse.parse_str(path).ok()?;
                let position = item.ident().span().start();
                if position.line == 1 {
                    let path = path.to_owned();
                    let hash = name.next().map(ToOwned::to_owned);
                    Some((path, hash, position.column))
                } else {
                    None
                }
            })
            .fold(source, |mut output, (path, hash, column)| {
                use std::fmt::Write;
                let _ = writeln!(output, "{path};");
                let position = Position::new(line as _, column as _);
                request.push(ItemRequest {
                    path,
                    hash,
                    position,
                });
                line += 1;
                output
            });

        let context = context.tap_mut(|c| c.push('}'));

        Self { context, request }
    }
}

struct ItemLinks {
    links: HashMap<String, client::ExternalDocLinks>,
    local: bool,
}

impl ItemLinks {
    fn get(&self, key: &str) -> Option<&str> {
        self.links
            .get(key)
            .and_then(|links| {
                if self.local {
                    links.local.as_ref()
                } else {
                    links.web.as_ref()
                }
            })
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
