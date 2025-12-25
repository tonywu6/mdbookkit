#![warn(clippy::unwrap_used)]

use std::{collections::HashMap, io::Write};

use anyhow::{
    Context,
    // not shadowing Result because it is linked from docs
    Result as Result2,
};
use clap::{Parser, Subcommand};
use futures_util::TryFutureExt;
use mdbook_preprocessor::PreprocessorContext;
use tap::{Pipe, Tap};
use tracing::{Level, debug, info, info_span, warn};

use mdbookkit::{
    book::{BookConfigHelper, BookHelper, book_from_stdin, string_from_stdin},
    diagnostics::Issue,
    emit_debug, emit_error, emit_trace, emit_warning,
    error::{ExitProcess, FutureWithError},
    logging::Logging,
};

use self::{
    cache::{Cache, FileCache},
    client::Client,
    env::{Config, Environment, RustAnalyzer},
    link::{LinkState, diagnostic::LinkStatus},
    page::Pages,
    resolver::Resolver,
};

mod cache;
mod client;
mod env;
mod item;
mod link;
mod markdown;
mod page;
mod resolver;
mod sync;
#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() {
    Logging::default().init();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    match Program::parse().command {
        Some(Command::Supports { .. }) => Ok(()),
        Some(Command::Markdown(options)) => markdown(options).await,
        Some(Command::RustAnalyzer) => which(),
        #[cfg(feature = "_testing")]
        Some(Command::Describe) => describe(),
        None => mdbook().await,
    }
    .exit(emit_error!())
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

    /// Support command for mdBook.
    ///
    /// See <https://rust-lang.github.io/mdBook/for_developers/preprocessors.html#hooking-into-mdbook>
    #[clap(hide = true)]
    Supports { renderer: String },

    #[cfg(feature = "_testing")]
    #[clap(hide = true)]
    Describe,
}

async fn mdbook() -> Result2<()> {
    let (ctx, mut book) = book_from_stdin().context("Failed to read from mdBook")?;

    let config = config(&ctx).context("Failed to read preprocessor config from book.toml")?;

    let client = Environment::new(config)
        .context("Failed to initialize preprocessor")?
        .tap(emit_debug!("{:#?}"))
        .pipe(Client::new);

    let cached = FileCache::load(client.env())
        .context("Could not load cache")
        .inspect_ok(emit_trace!("cache loaded: {:?}"))
        .inspect_err(emit_debug!())
        .await
        .ok();

    let mut content = Pages::default();

    for (path, ch) in book.iter_chapters() {
        let stream = client.env().markdown(&ch.content).into_offset_iter();
        content
            .read(path.clone(), &ch.content, stream)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
    }

    if let Some(cached) = cached {
        info!("Reusing cached items");
        cached.resolve(&mut content).await.ok();
    }

    client.resolve(&mut content).await?;

    let env = client.stop().await;

    let status = content
        .reporter()
        .name_display(|path| path.display().to_string())
        .build()
        .to_stderr()
        .to_status();

    link_report(&content);

    match status {
        LinkStatus::Unresolved => {
            if env.config.cache_dir.is_some() {
                warn! { "The `cache-dir` option is enabled, but some items could not \
                be resolved, which will cause rust-analyzer to always run \
                despite the cache." }
            }
        }
        LinkStatus::Ok | LinkStatus::Debug => {
            info!("Finished");
        }
    }

    // bail before emitting changes
    env.config.fail_on_warnings.check(status.level())?;

    if content.modified() {
        FileCache::save(&env, &content)
            .context("Failed to save cache")
            .inspect_err(emit_warning!())
            .await
            .ok();
    }

    let mut result = book
        .iter_chapters()
        .map(|(path, _)| {
            let _span = info_span!("emit", key = ?path).entered();
            debug!("generating output");
            let output = content
                .emit(path, &env.emit_config())
                .context("Error generating output")?;
            Ok((path.clone(), output))
        })
        .collect::<Result2<HashMap<_, _>>>()?;

    book.for_each_text_mut(|path, content| {
        if let Some(output) = result.remove(path) {
            *content = output;
        }
    });

    book.to_stdout(&ctx)?;

    Ok(())
}

async fn markdown(config: Config) -> Result2<()> {
    let client = Environment::new(config)
        .context("Failed to initialize")?
        .pipe(Client::new);

    let cached = FileCache::load(client.env())
        .context("Could not load cache")
        .inspect_ok(emit_debug!())
        .inspect_err(emit_debug!())
        .await
        .ok();

    let source = string_from_stdin().context("Failed to read Markdown source from stdin")?;
    let stream = client.env().markdown(&source).into_offset_iter();

    let mut content = Pages::one(&source, stream).context("Failed to parse Markdown source")?;

    if let Some(cached) = cached {
        cached.resolve(&mut content).await.ok();
    }

    client.resolve(&mut content).await?;

    let env = client.stop().await;

    let status = content
        .reporter()
        .name_display(|_| "<stdin>".into())
        .build()
        .to_stderr()
        .to_status();

    link_report(&content);

    if content.modified() {
        FileCache::save(&env, &content).await.ok();
    }

    (content.get(&env.emit_config()).map(|emit| emit.to_string()))
        .and_then(|output| Ok(std::io::stdout().write_all(output.as_bytes())?))?;

    env.config.fail_on_warnings.check(status.level())?;

    Ok(())
}

fn link_report<K>(content: &Pages<'_, K>) {
    let mut iter = content.iter();

    let result = iter.deduped(|link| match link.state() {
        LinkState::Pending(..) => Some(None),
        LinkState::Resolved(links) => Some(Some(links.url())),
        LinkState::Unparsed => None,
    });

    info!("Converted {}", iter.stats().fmt_resolved());

    if tracing::enabled!(target: "link-report", Level::DEBUG) {
        for (item, link) in result
            .into_iter()
            .filter_map(|(k, v)| Some((k, v?)))
            .collect::<Vec<_>>()
            .tap_mut(|items| {
                items.sort_by(|(k1, u1), (k2, u2)| (k1.as_ref(), u1).cmp(&(k2.as_ref(), u2)));
            })
        {
            if let Some(link) = link {
                info!(target: "link-report", "{item} => {link}")
            } else {
                warn!(target: "link-report", "{item} => (unresolved)")
            }
        }
    }
}

fn config(ctx: &PreprocessorContext) -> Result2<Config> {
    let mut config =
        (ctx.config).preprocessor::<Config>(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])?;

    if let Some(path) = config.manifest_dir {
        config.manifest_dir = Some(ctx.root.join(path))
    } else {
        config.manifest_dir = Some(ctx.root.clone())
    }

    if let Some(path) = config.cache_dir {
        config.cache_dir = Some(ctx.root.join(path))
    }

    Ok(config)
}

fn which() -> Result2<()> {
    let env = Environment::new(Default::default())?;

    match env.which() {
        RustAnalyzer::Custom(cmd) => println!("Using a custom command for rust-analyzer: {cmd:?}"),
        RustAnalyzer::VsCode(cmd) => println!(
            "Using rust-analyzer from VS Code extension: {}",
            cmd.display()
        ),
        RustAnalyzer::Path => println!("Using rust-analyzer on PATH"),
    }

    Ok(())
}

#[cfg(feature = "_testing")]
fn describe() -> Result2<()> {
    print!("{}", mdbookkit::docs::describe_preprocessor::<Config>()?);
    Ok(())
}

static UNIQUE_ID: &str = "__ded48f4d_0c4f_4950_b17d_55fd3b2a0c86__";

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
