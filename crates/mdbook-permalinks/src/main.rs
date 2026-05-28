#![cfg_attr(not(test), warn(clippy::unwrap_used))]

use std::{collections::HashSet, fmt::Debug};

use anyhow::{Context, Result};
use git2::Repository;
use mdbook_markdown::pulldown_cmark;
use mdbook_preprocessor::{PreprocessorContext, book::Book};
use tap::{Pipe, Tap};
use tracing::{Level, debug, error_span, info, info_span, span::EnteredSpan, trace, warn};
use url::Url;

use mdbookkit::{
    book::{PreprocessorHelper, book_from_stdin},
    config::validate_config_examples,
    diagnostics::IssueReporter,
    emit, emit_error,
    env::is_logging,
    error::{ProgramExit, ReadableDebug, WithDebugContext, has_severity},
    level_enabled,
    logging::init_logging,
    ticker, ticker_item,
    url::{UrlFromPath, UrlSuffix, UrlUtil},
};

use self::{
    link::{ContentHint, LinkStatus, PathStatus, RelativeLink},
    options::Config,
    page::Pages,
    vcs::Permalink,
};

mod diagnostics;
mod link;
mod options;
mod page;
mod vcs;

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

fn mdbook() -> Result<(), ()> {
    let (ctx, book) = book_from_stdin()
        .context("failed to read from mdBook")
        .or_else(emit_error!())?;

    let book = match Environment::new(&ctx) {
        Ok(Ok(env)) => env.process(book)?,
        Ok(Err(err)) => {
            warn!("{:?}", err.context("preprocessor will be disabled"));
            book
        }
        Err(err) => Err(err)
            .context("failed to initialize preprocessor")
            .or_else(emit_error!())?,
    };

    ctx.print(book).or_else(emit_error!())?;

    Ok(())
}

#[derive(clap::Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    #[clap(hide = true)]
    Supports { renderer: String },
    #[clap(hide = true)]
    ValidateConfig,
}

struct Environment<'a> {
    ctx: &'a PreprocessorContext,
    vcs: VersionControl,
    page_dir: Url,
    markdown: pulldown_cmark::Options,
    config: Config,
}

struct VersionControl {
    root: Url,
    link: Permalink,
    repo: Repository,
}

impl Environment<'_> {
    fn process(self, mut book: Book) -> Result<Book, ()> {
        let mut contents = Pages::new(self.markdown);

        self.ctx.for_each_page(&book, |path, content| {
            info_span!("page_read", file = ?path.show()).in_scope(|| {
                (contents.insert(path, content))
                    .context("failed to parse file as markdown")
                    .or_else(emit_error!())
            })
        })?;

        self.resolve(&mut contents);

        for issues in self.issues(&contents).pipe(IssueReporter::sorted) {
            issues.emit(emit!());
        }

        contents.log_stats();

        // bail before emitting changes
        self.config
            .fail_on_warnings
            .check()
            .or_else(emit_error!())?;

        let mut contents = contents.emit();

        self.ctx.for_each_page_mut(&mut book, |path, content| {
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

        if has_severity(Level::WARN) {
            warn!("finished with warnings");
        } else {
            info!("finished");
        }

        Ok(book)
    }

    fn resolve(&self, content: &mut Pages<'_>) {
        let page_paths = &content.paths(&self.page_dir);

        let ticker = ticker!(Level::INFO, "process", "processing links").entered();

        for (base, link) in content.links_mut() {
            let file_url = match if let Some(link) = link.href.strip_prefix('/') {
                self.vcs.root.join(link)
            } else {
                base.join(&link.href)
            } {
                Ok(url) => url,
                Err(e) => {
                    debug!("ignoring unparsable link {:?}: {e}", &*link.href);
                    link.status = LinkStatus::Ignored;
                    continue;
                }
            };

            let env = self;
            let page_url = base.as_ref();

            if let Some(book) = &env.config.book_url
                && let Some(path) = book.as_ref().make_relative(&file_url)
                && !path.starts_with("../")
            {
                let dest = ResolveBook {
                    link,
                    file_url,
                    page_url,
                    page_paths,
                    path,
                };
                let _span = dest.span(&ticker);
                self.resolve_book(dest);
            } else if let Some((path, hint)) = env.vcs.link.to_path(&file_url)
                && let Ok(url) = env.vcs.root.join(&path)
            {
                let (_, url_suffix) = env.vcs.link.as_ref().remove_suffix(file_url);
                let dest = ResolveFile {
                    hint,
                    url_suffix,
                    check_mode: true,
                    file_url: url,
                    page_url,
                    page_paths,
                    link,
                };
                let _span = dest.span(&ticker);
                self.resolve_file(dest);
            } else if file_url.scheme() == "file" {
                let (file_url, url_suffix) = env.vcs.link.as_ref().remove_suffix(file_url);
                let dest = ResolveFile {
                    hint: link.hint,
                    url_suffix,
                    check_mode: false,
                    file_url,
                    page_url,
                    page_paths,
                    link,
                };
                let _span = dest.span(&ticker);
                self.resolve_file(dest);
            }
        }
    }

    fn resolve_file(
        &self,
        ResolveFile {
            file_url,
            page_url,
            page_paths,
            hint,
            url_suffix,
            check_mode,
            link,
        }: ResolveFile,
    ) {
        let relative_to_repo = self.vcs.try_file(&file_url);

        let relative_to_book = (self.page_dir)
            .make_relative(&file_url)
            .expect("both are file urls");

        trace!(?relative_to_book);

        let not_in_book = relative_to_book.starts_with("../");

        // the markdown file is in book src/ but not in SUMMARY.md
        let not_in_tree =
            relative_to_book.ends_with(".md") && !page_paths.contains(&relative_to_book);

        let always_link = (self.config.always_link)
            .iter()
            .any(|suffix| file_url.path().ends_with(suffix));

        let should_link = check_mode || not_in_book || not_in_tree || always_link;

        trace! { should_link, check_mode, not_in_book, not_in_tree, always_link };

        if !should_link {
            if let Err(
                err @ (PathStatus::NotFound
                | PathStatus::NotADirectory
                | PathStatus::Unreachable
                | PathStatus::NotInRepo),
            ) = relative_to_repo
            {
                // at this point `not_in_book` is false
                // it is okay for `err` to be `Ignored` because the file
                // will be copied to output anyway
                link.unreachable(vec![(file_url, err)]);
            } else if link.href.starts_with('/') {
                // mdBook doesn't support absolute paths like VS Code does
                let file_url = url_suffix.restored(file_url);
                let rewritten = page_url
                    .make_relative(&file_url)
                    .expect("both are file urls");
                link.rewritten(rewritten);
            } else {
                link.unchanged();
            }
        } else {
            match relative_to_repo {
                Ok(file) => {
                    let href = self.vcs.link.to_link(&file.link, hint);
                    let href = url_suffix.restored(href).as_str().to_owned();
                    link.permalink(href);
                }
                Err(err) => {
                    link.unreachable(vec![(file_url, err)]);
                }
            }
        }
    }

    /// Check hard-coded URLs to book content
    fn resolve_book(
        &self,
        ResolveBook {
            file_url,
            page_url,
            page_paths,
            path,
            link,
        }: ResolveBook,
    ) {
        let path = {
            let mut path = path;
            trace!(?path);
            if let Some(idx) = path.find('#') {
                path.truncate(idx);
                trace!(?path, "removing fragment");
            };
            if let Some(idx) = path.find('?') {
                path.truncate(idx);
                trace!(?path, "removing query");
            };
            trace!(?path);
            path
        };

        let mut not_found = vec![];

        let is_index = path.is_empty() || path.ends_with('/');

        trace!(is_index);

        // one does not simply avoid trailing slash issues...
        // https://github.com/slorber/trailing-slash-guide
        let try_pages: &[String] = if is_index {
            // enforce that index.html pages should consistently
            // be addressed with a trailing slash
            &[format!("{path}index.md"), format!("{path}README.md")]
        } else if let Some(path) = path.strip_suffix(".html") {
            // expect a `*.html` link to point to a `*.md` file
            // note that because `.html` is explicitly specified here,
            // index pages are not considered
            &[format!("{path}.md")]
        } else {
            // this is a path without an extension
            &[
                format!("{path}.md"),
                // all major hosting providers implicitly redirect
                // /folder to /folder/, so these are okay
                format!("{path}/index.md"),
                format!("{path}/README.md"),
            ]
        };

        for page in try_pages {
            trace!("trying book page {page:?}");

            let file_url = (self.page_dir)
                .join(page)
                .with_debug(&**page, "page")
                .expect("`page` should be parsable as a url path")
                .tap_mut(|u| u.set_query(file_url.query()))
                .tap_mut(|u| u.set_fragment(file_url.fragment()));

            if page_paths.contains(page) {
                let rewritten = page_url
                    .make_relative(&file_url)
                    .expect("both are file urls");
                link.rewritten(rewritten);
                return;
            }

            trace!("not found: {file_url}");
            not_found.push((file_url, PathStatus::NotInBook));
        }

        if !is_index {
            // try the unmodified path itself
            let try_file = (self.page_dir)
                .join(&path)
                .with_debug(&*path, "path")
                .expect("`path` should be parsable as a url path");

            match self.vcs.try_file(&try_file) {
                Ok(result) if !result.metadata.is_dir() => {
                    let file_url = try_file
                        .tap_mut(|u| u.set_query(file_url.query()))
                        .tap_mut(|u| u.set_fragment(file_url.fragment()));

                    let rewritten = page_url
                        .make_relative(&file_url)
                        .expect("both are file urls");

                    link.rewritten(rewritten);

                    return;
                }
                Ok(_) => {
                    // a directory may exist but not accessible
                    // due to having no index.html
                    not_found.push((try_file, PathStatus::NotInBook));
                }
                Err(err) => {
                    not_found.push((try_file, err));
                }
            }
        }

        link.unreachable(not_found);
    }
}

impl<'a> Environment<'a> {
    fn new(ctx: &'a PreprocessorContext) -> Result<Result<Self>> {
        let config = ctx
            .preprocessor(&[PREPROCESSOR_NAME, "mdbook-link-forever"])
            .inspect(|c| debug!("{c:#?}"))
            .context("failed to read preprocessor config from book.toml")?;

        let vcs = match VersionControl::try_from_git(&config, ctx) {
            Ok(Ok(vcs)) => vcs,
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };

        let markdown = ctx.markdown_options();

        let page_dir = ctx.page_dir()?.dir_to_url();

        Ok(Ok(Self {
            ctx,
            vcs,
            page_dir,
            markdown,
            config,
        }))
    }
}

struct ResolveFile<'a, 'r> {
    file_url: Url,
    page_url: &'a Url,
    page_paths: &'a HashSet<String>,
    hint: ContentHint,
    url_suffix: UrlSuffix,
    /// the link was written as an http url rather than a path
    check_mode: bool,
    link: &'a mut RelativeLink<'r>,
}

struct ResolveBook<'a, 'r> {
    file_url: Url,
    page_url: &'a Url,
    page_paths: &'a HashSet<String>,
    path: String,
    link: &'a mut RelativeLink<'r>,
}

impl<'a, 'r> ResolveFile<'a, 'r> {
    #[inline]
    fn span(&self, ticker: &tracing::Span) -> EnteredSpan {
        let Self {
            file_url,
            page_url,
            hint,
            link,
            ..
        } = self;
        if !is_logging() {
            ticker_item!(ticker, Level::INFO, "file_link", "{:?}", &*link.href)
        } else if level_enabled!(Level::TRACE) {
            ticker_item! {
                ticker, Level::TRACE, "file_link",
                %file_url, %page_url, ?hint,
                "{:?}", &*link.href
            }
        } else {
            ticker_item!(ticker, Level::DEBUG, "file_link", "{:?}", &*link.href)
        }
        .entered()
    }
}

impl<'a, 'r> ResolveBook<'a, 'r> {
    #[inline]
    fn span(&self, ticker: &tracing::Span) -> EnteredSpan {
        let Self {
            file_url,
            page_url,
            path,
            link,
            ..
        } = self;
        if !is_logging() {
            ticker_item!(ticker, Level::INFO, "book_link", "{:?}", &*link.href)
        } else if level_enabled!(Level::TRACE) {
            ticker_item! {
                ticker, Level::TRACE, "book_link",
                %file_url, %page_url, ?path,
                "{:?}", &*link.href
            }
        } else {
            ticker_item!(ticker, Level::DEBUG, "book_link", "{:?}", &*link.href)
        }
        .entered()
    }
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
