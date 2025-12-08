use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    io::{Read, Write},
    sync::Arc,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use lsp_types::Position;
use mdbook::preprocess::PreprocessorContext;
use tap::{Pipe, TapFallible};
use tokio::task::JoinSet;

use mdbookkit::{
    book::{
        book_from_stdin, book_into_stdout, config_from_book, for_each_chapter_mut, iter_chapters,
        smart_punctuation,
    },
    diagnostics::Issue,
    log_debug, log_warning,
    logging::{ConsoleLogger, is_logging, spinner},
    styled,
};

use self::{
    cache::{Cache, FileCache},
    client::Client,
    env::{Config, Environment, RustAnalyzer},
    item::Item,
    link::ItemLinks,
    page::Pages,
    url::UrlToPath,
};

mod cache;
mod client;
mod env;
mod item;
mod link;
mod markdown;
mod page;
mod sync;
#[cfg(test)]
mod tests;
mod url;

/// Type that can provide links.
///
/// Resolvers should modify the provided [`Pages`] in place.
///
/// This is currently an abstraction over two sources of links:
///
/// - [`Client`], which invokes rust-analyzer
/// - [`Cache`] implementations
///
/// [`Cache`]: crate::bin::rustdoc_link::cache::Cache
trait Resolver {
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result<()>
    where
        K: Eq + Hash;
}

impl Resolver for Client {
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result<()>
    where
        K: Eq + Hash,
    {
        let request = pages.items();

        if request.is_empty() {
            return Ok(());
        }

        let main = std::fs::read_to_string(self.env().entrypoint.to_path()?)?;

        let (context, request) = {
            let mut context = format!("{main}\nfn {UNIQUE_ID} () {{\n");

            let line = context.chars().filter(|&c| c == '\n').count();

            let request = request
                .iter()
                .scan(line, |line, (key, item)| {
                    build(&mut context, line, item).map(|cursors| (key.clone(), cursors))
                })
                .collect::<Vec<_>>();

            fn build(context: &mut String, line: &mut usize, item: &Item) -> Option<Vec<Position>> {
                use std::fmt::Write;
                let _ = writeln!(context, "{}", item.stmt);
                let cursors = item
                    .cursor
                    .as_ref()
                    .iter()
                    .map(|&col| Position::new(*line as _, col as _))
                    .collect::<Vec<_>>();
                *line += 1;
                Some(cursors)
            }

            context.push('}');

            (context, request)
        };

        log::debug!("request context\n\n{context}\n");

        let document = self
            .open(self.env().entrypoint.clone(), context)
            .await?
            .pipe(Arc::new);

        spinner().create("resolve", Some(request.len() as _));

        let tasks: JoinSet<Option<(String, ItemLinks)>> = request
            .into_iter()
            .map(|(key, pos)| {
                let key = key.to_string();
                let doc = document.clone();
                resolve(doc, key, pos)
            })
            .collect();

        async fn resolve(
            doc: Arc<client::OpenDocument>,
            key: String,
            pos: Vec<Position>,
        ) -> Option<(String, ItemLinks)> {
            let _task = spinner().task("resolve", &key);
            for p in pos {
                let resolved = doc
                    .resolve(p)
                    .await
                    .with_context(|| format!("{p:?}"))
                    .context("failed to resolve symbol:")
                    .tap_err(log_debug!())
                    .ok();
                if let Some(resolved) = resolved {
                    return Some((key, resolved));
                }
            }
            None
        }

        let resolved = tasks
            .join_all()
            .await
            .into_iter()
            .flatten()
            .collect::<HashMap<_, _>>();

        spinner().finish("resolve", styled!(("done").green()));

        pages.apply(&resolved);

        Ok(())
    }
}

impl<K> Resolver for HashMap<K, ItemLinks>
where
    K: Borrow<str> + Eq + Hash,
{
    async fn resolve<P>(&self, pages: &mut Pages<'_, P>) -> Result<()>
    where
        P: Eq + Hash,
    {
        pages.apply(self);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    ConsoleLogger::install(env!("CARGO_PKG_NAME"));
    match Program::parse().command {
        Some(Command::Supports { .. }) => Ok(()),
        Some(Command::Markdown(options)) => markdown(options).await,
        Some(Command::RustAnalyzer) => which(),
        #[cfg(feature = "_testing")]
        Some(Command::Describe) => describe(),
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
    /// Convert rustdoc-style Markdown links found in stdin.
    Markdown(Config),

    /// Show which `rust-analyzer` is being used.
    RustAnalyzer,

    /// Support command for mdbook.
    ///
    /// See <https://rust-lang.github.io/mdBook/for_developers/preprocessors.html#hooking-into-mdbook>
    #[clap(hide = true)]
    Supports { renderer: String },

    #[cfg(feature = "_testing")]
    #[clap(hide = true)]
    Describe,
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

fn which() -> Result<()> {
    let env = Environment::new(Default::default())?;

    match env.which() {
        RustAnalyzer::Custom(cmd) => println!("using a custom command for rust-analyzer: {cmd:?}"),
        RustAnalyzer::VsCode(cmd) => println!(
            "using rust-analyzer from VS Code extension: {}",
            cmd.display()
        ),
        RustAnalyzer::Path => println!("using rust-analyzer on PATH (run `which rust-analyzer`)"),
    }

    Ok(())
}

#[cfg(feature = "_testing")]
fn describe() -> Result<()> {
    print!("{}", mdbookkit::docs::describe_preprocessor::<Config>()?);
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

fn string_from_stdin() -> Result<String> {
    Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(|buf| Ok(String::from_utf8(buf)?))
}

const UNIQUE_ID: &str = "__ded48f4d_0c4f_4950_b17d_55fd3b2a0c86__";
