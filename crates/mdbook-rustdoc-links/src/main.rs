#![cfg_attr(not(test), warn(clippy::unwrap_used))]

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{Level, debug, error_span, info, info_span, warn};

use mdbookkit::{
    book::{PreprocessorHelper, book_from_stdin},
    config::validate_config_examples,
    diagnostics::IssueReporter,
    emit, emit_error, emit_warning,
    error::{ProgramExit, Show, WithDebugContext, has_severity},
    logging::init_logging,
};

use self::{
    builder::build_docs,
    env::Environment,
    options::Config,
    tracker::{ExportedPages, LinkTracker},
};

mod builder;
mod diagnostics;
mod env;
mod markdown;
mod options;
mod subprocess;
mod tracker;

fn main() {
    init_logging();
    let _span = error_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Some(Command::Supports { .. }) => Ok(()),
        Some(Command::ValidateConfig) => {
            validate_config_examples::<Config>(PREPROCESSOR_NAME).or_else(emit_error!())
        }

        None => mdbook(),
    }
    .exit()
}

#[derive(clap::Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    /// Support command for mdBook.
    ///
    /// See <https://rust-lang.github.io/mdBook/for_developers/preprocessors.html#hooking-into-mdbook>
    #[clap(hide = true)]
    Supports { renderer: String },
    #[clap(hide = true)]
    ValidateConfig,
}

#[derive(clap::Parser, Debug, Clone)]
struct MarkdownCommand {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[clap(required(true))]
    files: Vec<PathBuf>,
}

fn mdbook() -> Result<(), ()> {
    let (ctx, mut book) = book_from_stdin()
        .context("failed to read from mdBook")
        .or_else(emit_error!())?;

    let Config {
        builder,
        env,
        fail_on_warnings,
    } = ctx
        .book_toml()
        .preprocessor(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])
        .inspect(|c| debug!("{c:#?}"))
        .context("failed to read preprocessor config from book.toml")
        .or_else(emit_error!())?
        .unwrap_or_default();

    let env = Environment::new(env, &ctx).or_else(emit_error!())?;

    let mut tracker = LinkTracker::new(env);

    ctx.for_each_page(&book, |path, content| {
        info_span!("page_read", file = ?path.show()).in_scope(|| {
            tracker
                .read(content, path)
                .context("failed to parse file as markdown")
                .or_else(emit_error!())?;
            Ok(())
        })
    })?;

    build_docs(builder.resolve(tracker.env().book_dir())?, &mut tracker)?;

    let ExportedPages {
        mut contents,
        issues,
        stats,
    } = tracker.export();

    for issues in IssueReporter::sorted(issues) {
        issues.emit(emit!());
    }

    tracker.symlink_docs().or_else(emit_warning!()).ok();

    fail_on_warnings.check().or_else(emit_error!())?;

    ctx.for_each_page_mut(&mut book, |path, content| {
        let text = contents
            .remove(&path)
            .with_debug(&path, "file")
            .expect("`contents` should contain path");

        *content = text
            .with_debug(&path, "file")
            .context("error generating output for file")
            .or_else(emit_error!())?;

        Ok(())
    })?;

    ctx.print(book).or_else(emit_error!())?;

    info!("{stats}");

    if has_severity(Level::WARN) {
        warn!("finished with warnings");
    } else {
        info!("finished");
    }

    Ok(())
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
