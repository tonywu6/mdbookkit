use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use lsp_types::Position;
use serde::Deserialize;
use tap::{Pipe, TapFallible};
use tokio::task::JoinSet;

use crate::{
    cache::{Cache, Cacheable},
    client::{Client, ItemLinks},
    env::Environment,
    item::{Carets, Item},
    logger::spinner,
    markdown::{markdown_parser, Page},
};

pub mod cache;
pub mod client;
pub mod env;
pub mod item;
pub mod logger;
pub mod markdown;
mod sync;

#[derive(clap::Parser, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct BuildOptions {
    #[arg(long)]
    #[serde(default)]
    pub rust_analyzer: Option<String>,

    #[arg(long)]
    #[serde(default)]
    pub manifest_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    pub smart_punctuation: bool,

    #[arg(long)]
    #[serde(default)]
    pub prefer_local_links: bool,
}

pub async fn process(content: &str, options: BuildOptions) -> Result<String> {
    let client = Client::new(Environment::new(options)?);

    let stream =
        markdown_parser(content, client.env.build_opts.smart_punctuation).into_offset_iter();

    let (page, request) = Page::read(content, stream)?;

    let request = Item::parse_all(request.iter());

    let symbols = Client::request.cached::<Cache>(&client, request).await?;

    let BuildOptions {
        prefer_local_links, ..
    } = client.env.build_opts;

    client.dispose().await?;

    page.emit(|k| symbols.get(k, prefer_local_links))
}

impl Client {
    pub async fn request(&self, request: Vec<Item>) -> Result<Resolved> {
        let src = std::fs::read_to_string(self.env.entrypoint.path())?;

        let request = Request::new(&src, request);

        let mut items = HashMap::new();

        if request.request.is_empty() {
            return Ok(Resolved { items });
        }

        log::trace!("request context\n\n{}\n", request.context);

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

                let _task = spinner().task("resolve", &item.key);

                for &p in positions {
                    let links = document
                        .resolve(p)
                        .await
                        .context("error while resolving external docs")
                        .tap_err(log_debug!())
                        .unwrap_or_default();

                    log::trace!("resolve {} {links:#?}", item.key);

                    if !links.is_empty() {
                        let links = links.with_fragment(item.fragment.as_deref());
                        return Ok((item, links));
                    }
                }

                Ok((item, Default::default()))
            });
        }

        while let Some(res) = tasks.join_next().await {
            let Ok(Ok((item, links))) = res else {
                continue;
            };
            if links.is_empty() {
                log::warn!("failed to resolve links for {:?}", item.key);
                continue;
            }
            items.insert(item.key, links);
        }

        spinner().finish("resolve", "done");

        Ok(Resolved { items })
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
pub struct Resolved {
    items: HashMap<String, ItemLinks>,
}

impl Resolved {
    pub fn get(&self, key: &str, local: bool) -> Option<&str> {
        let sym = self.items.get(key)?;
        if local {
            sym.local.as_ref()
        } else {
            sym.web.as_ref()
        }
        .map(|u| u.as_str())
    }
}

pub fn preprocessor_name() -> &'static str {
    let name = env!("CARGO_PKG_NAME");
    if let Some(idx) = name.find('-') {
        &name[idx + 1..]
    } else {
        name
    }
}
