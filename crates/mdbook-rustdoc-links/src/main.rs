#![warn(clippy::unwrap_used)]

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{Level, info, info_span, warn};

use mdbookkit::{
    book::{BookHelper, PreprocessorHelper, book_from_stdin},
    emit_error,
    error::{ExitProcess, has_severity},
    logging::Logging,
};

use self::{builder::build_docs, diagnostic::Reporter, options::Config, tracker::LinkTracker};

mod builder;
mod diagnostic;
mod markdown;
mod options;
// #[cfg(test)]
// mod tests;
mod tracker;

fn main() {
    Logging::default().init();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    match Program::parse().command {
        Some(Command::Supports { .. }) => Ok(()),
        None => mdbook(),
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
    /// Support command for mdBook.
    ///
    /// See <https://rust-lang.github.io/mdBook/for_developers/preprocessors.html#hooking-into-mdbook>
    #[clap(hide = true)]
    Supports { renderer: String },
}

fn mdbook() -> Result<()> {
    let (ctx, mut book) = book_from_stdin().context("Failed to read from mdBook")?;

    let Config {
        build,
        fail_on_warnings,
    } = ctx
        .preprocessor(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])
        .context("Failed to read preprocessor config from book.toml")?;

    let mut contents = LinkTracker::default();

    let keys = book
        .iter_chapters()
        .map(|(path, chapter)| {
            (contents.read(&chapter.content))
                .with_context(|| path.display().to_string())
                .context("Failed to parse file as Markdown:")?;
            Ok(path.clone())
        })
        .collect::<Result<Vec<_>>>()?;

    build_docs(build, &mut contents)?;

    let results = contents.export()?;

    for ((path, chapter), issues) in book.iter_chapters().zip(results.issues) {
        Reporter {
            issues,
            source: (&*chapter.content, path.display()),
            tracer: emit_issue!(),
        }
        .emit();
    }

    fail_on_warnings.check()?;

    let mut contents = keys
        .into_iter()
        .zip(results.contents)
        .collect::<HashMap<_, _>>();

    book.for_each_page_mut(|path, content| {
        if let Some(rendered) = contents.remove(path) {
            *content = rendered
        }
        Ok(())
    })?;

    book.to_stdout(&ctx)?;

    if has_severity(Level::WARN) {
        warn!("Finished with warnings");
    } else {
        info!("Finished");
    }

    Ok(())
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
