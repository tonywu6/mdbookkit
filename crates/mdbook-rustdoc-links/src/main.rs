#![warn(clippy::unwrap_used)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{Level, error_span, info, info_span, warn};

use mdbookkit::{
    book::{BookHelper, PreprocessorHelper, book_from_stdin, utf8_path},
    config::validate_config_examples,
    diagnostics::IssueReporter,
    emit_error, emit_warning,
    error::{PathDebug, ProgramExit, has_severity},
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
        .preprocessor(&[PREPROCESSOR_NAME, "mdbook-rustdoc-link"])
        .context("failed to read preprocessor config from book.toml")
        .or_else(emit_error!())?;

    let env = Environment::new(env, &ctx).or_else(emit_error!())?;

    let mut tracker = LinkTracker::new(env);

    for (path, ch) in book.iter_chapters() {
        info_span!("page_read", path = ?path.debug()).in_scope(|| {
            let path = utf8_path(path).or_else(emit_error!())?;
            tracker
                .read(&ch.content, path)
                .context("failed to parse file as markdown")
                .or_else(emit_error!())?;
            Ok(())
        })?;
    }

    build_docs(builder.resolve(tracker.env().book_dir())?, &mut tracker)?;

    let ExportedPages {
        mut contents,
        issues,
        stats,
    } = tracker.export();

    for issues in IssueReporter::sorted(issues) {
        issues.emit();
    }

    fail_on_warnings.check().or_else(emit_error!())?;

    tracker.symlink_docs().or_else(emit_warning!()).ok();

    book.for_each_page_mut(|path, content| {
        let key = path.to_str().expect("paths were checked");
        let out = contents.remove(key).expect("`contents` should have key");

        *content = out
            .with_context(|| key.to_owned())
            .context("error generating output")
            .or_else(emit_error!())?;

        Ok(())
    })?;

    book.to_stdout(&ctx).or_else(emit_error!())?;

    info!("{stats}");

    if has_severity(Level::WARN) {
        warn!("finished with warnings");
    } else {
        info!("finished");
    }

    Ok(())
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
