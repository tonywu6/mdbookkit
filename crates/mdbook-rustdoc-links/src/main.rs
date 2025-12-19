use std::{collections::HashMap, io::Write};

use anyhow::{
    Context,
    // not shadowing Result because it is linked from docs
    Result as Result2,
};
use clap::{Parser, Subcommand};
use console::colors_enabled_stderr;
use log::LevelFilter;
use mdbook_preprocessor::PreprocessorContext;
use tap::{Pipe, TapFallible};

use mdbookkit::{
    book::{BookConfigHelper, BookHelper, book_from_stdin, string_from_stdin},
    diagnostics::Issue,
    log_warning,
    logging::{ConsoleLogger, is_logging},
};

use self::{
    cache::{Cache, FileCache},
    client::Client,
    env::{Config, Environment, RustAnalyzer},
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
mod url;

#[tokio::main]
async fn main() -> Result2<()> {
    ConsoleLogger::install(PREPROCESSOR_NAME);
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

async fn mdbook() -> Result2<()> {
    let (ctx, mut book) = book_from_stdin().context("failed to read from mdbook")?;

    let config = config(&ctx).context("failed to read preprocessor config from book.toml")?;

    let client = Environment::new(config)
        .context("failed to initialize `mdbook-rustdoc-link`")?
        .pipe(Client::new);

    let cached = FileCache::load(client.env()).await.ok();

    let mut content = Pages::default();

    for (path, ch) in book.iter_chapters() {
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

    let mut result = book
        .iter_chapters()
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

    book.for_each_text_mut(|path, content| {
        if let Some(output) = result.remove(path) {
            *content = output;
        }
    });

    book.to_stdout(&ctx)?;

    env.config.fail_on_warnings.check(status.level())?;

    Ok(())
}

async fn markdown(config: Config) -> Result2<()> {
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

fn which() -> Result2<()> {
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
fn describe() -> Result2<()> {
    print!("{}", mdbookkit::docs::describe_preprocessor::<Config>()?);
    Ok(())
}

fn config(ctx: &PreprocessorContext) -> Result2<Config> {
    let mut config = ctx
        .config
        .preprocessor::<Config>(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])?;

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

static UNIQUE_ID: &str = "__ded48f4d_0c4f_4950_b17d_55fd3b2a0c86__";

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
