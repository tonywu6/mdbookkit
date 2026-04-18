#![warn(clippy::unwrap_used)]

use std::collections::HashMap;

use anyhow::Context;
use cargo_metadata::camino::Utf8Path;
use clap::{Parser, Subcommand};
use tap::Pipe;
use tracing::{Level, info, info_span, warn};

use mdbookkit::{
    book::{BookHelper, PreprocessorHelper, book_from_stdin},
    diagnostics::IssueReporter,
    emit,
    error::{Break, ConsumeError, PathDebug, ProgramExit, has_severity},
    logging::init_logging,
};

use self::{
    builder::build_docs,
    options::Config,
    tracker::{ExportedPages, LinkTracker},
};

mod builder;
mod diagnostics;
mod markdown;
mod options;
mod subprocess;
mod tracker;

fn main() {
    init_logging();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    match Program::parse().command {
        Some(Command::Supports { .. }) => Ok(()),
        None => mdbook(),
    }
    .exit()
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

fn mdbook() -> Result<(), Break> {
    let (ctx, mut book) = book_from_stdin()
        .context("failed to read from mdBook")
        .or_error(emit!())?;

    let Config {
        build,
        fail_on_warnings,
    } = ctx
        .preprocessor(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])
        .context("failed to read preprocessor config from book.toml")
        .or_error(emit!())?;

    let mut contents = LinkTracker::default();

    let keys = book
        .iter_chapters()
        .map(|(path, chapter)| {
            info_span!("page_read", path = ?path.debug()).in_scope(|| {
                contents
                    .read(&chapter.content)
                    .context("failed to parse file as markdown")
                    .or_error(emit!())?;
                Ok(path.clone())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let book_dir = <&Utf8Path>::try_from(&*ctx.root)
        .context("book directory path contains non-UTF-8 characters, which is unsupported")
        .or_error(emit!())?;

    build_docs(build.resolve(book_dir)?, &mut contents)?;

    let ExportedPages {
        contents,
        issues,
        stats,
    } = contents.export();

    {
        let issues = (book.iter_chapters().zip(issues))
            .map(|((path, chapter), issues)| IssueReporter {
                issues,
                source: (&*chapter.content, path.display()).into(),
            })
            .collect::<Vec<_>>()
            .pipe(IssueReporter::sorted);

        for issues in issues {
            issues.emit();
        }
    }

    fail_on_warnings.check().or_error(emit!())?;

    {
        let mut contents = keys.into_iter().zip(contents).collect::<HashMap<_, _>>();

        book.for_each_page_mut(|path, content| {
            if let Some(output) = contents.remove(path) {
                *content = output
                    .with_context(|| path.display().to_string())
                    .context("error generating output")
                    .or_error(emit!())?;
            }
            Ok(())
        })?;
    }

    book.to_stdout(&ctx).or_error(emit!())?;

    info!("{stats}");

    if has_severity(Level::WARN) {
        warn!("finished with warnings");
    } else {
        info!("finished");
    }

    Ok(())
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
