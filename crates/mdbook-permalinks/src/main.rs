#![warn(clippy::unwrap_used)]

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    str::FromStr,
};

use anyhow::{Context, Result};
use git2::Repository;
use mdbook_markdown::pulldown_cmark;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext, book::Book};
use serde::Deserialize;
use tap::Tap;
use tracing::{Level, debug, info, info_span, span::EnteredSpan, trace, warn};
use url::Url;

use mdbookkit::{
    book::{BookConfigHelper, BookHelper, book_from_stdin},
    diagnostics::Issue,
    emit_debug, emit_error,
    error::{ExitProcess, OnWarning},
    logging::Logging,
    ticker, ticker_item,
    url::{ExpectUrl, UrlFromPath},
};

use self::{
    link::{ContentHint, LinkStatus, PathStatus, RelativeLink},
    page::Pages,
    vcs::{Permalink, PermalinkFormat},
};

mod diagnostic;
mod link;
mod page;
#[cfg(test)]
mod tests;
mod vcs;

fn main() -> Result<()> {
    Logging::default().init();
    let _span = info_span!({ env!("CARGO_PKG_NAME") }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        None => mdbook().exit(emit_error!()),
        Some(Command::Supports { .. }) => Ok(()),
        #[cfg(feature = "_testing")]
        Some(Command::Describe) => {
            print!("{}", mdbookkit::docs::describe_preprocessor::<Config>()?);
            Ok(())
        }
    }
}

fn mdbook() -> Result<()> {
    let (ctx, book) = book_from_stdin().context("Failed to read from mdBook")?;
    Permalinks.run(&ctx, book)?.to_stdout(&ctx)?;
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
    #[cfg(feature = "_testing")]
    #[clap(hide = true)]
    Describe,
}

struct Permalinks;

impl Preprocessor for Permalinks {
    fn name(&self) -> &str {
        PREPROCESSOR_NAME
    }

    fn run(&self, ctx: &PreprocessorContext, book: Book) -> Result<Book> {
        match Environment::new(ctx) {
            Ok(Ok(env)) => {
                debug!("{env:#?}");
                env.run(ctx, book)
            }
            Ok(Err(err)) => {
                warn!("{:?}", err.context("Preprocessor will be disabled"));
                Ok(book)
            }
            Err(err) => Err(err).context("Failed to initialize"),
        }
    }
}

struct Environment {
    vcs: VersionControl,
    root_dir: Url,
    markdown: pulldown_cmark::Options,
    config: Config,
}

struct VersionControl {
    root: Url,
    link: Permalink,
    repo: Repository,
}

impl Preprocessor for Environment {
    fn name(&self) -> &str {
        PREPROCESSOR_NAME
    }

    fn run(&self, _: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let mut content = Pages::new(self.markdown);

        for (path, ch) in book.iter_chapters() {
            let path = path
                .to_str()
                .context("only Unicode characters are supported")
                .with_context(|| format!("{path:?} contains unsupported characters"))?;
            let url = self.root_dir.join(path).expect_url();
            content
                .insert(url, &ch.content)
                .with_context(|| format!("Failed to parse {path:?}"))?;
        }

        self.resolve(&mut content);

        let status = self
            .reporter(&content, |_| true)
            .name_display(|url| self.rel_path(url))
            .build()
            .to_stderr()
            .to_status();

        content.log_stats();

        // bail before emitting changes
        self.config.fail_on_warnings.check(status.level())?;

        let mut result = book
            .iter_chapters()
            .map(|(path, _)| {
                let _span = info_span!("emit", key = ?path).entered();
                debug!("generating output");
                let key = path.to_str().expect("paths have been checked");
                let url = self.root_dir.join(key).expect_url();
                let out = content.emit(&url).context("Error generating output")?;
                Ok((path.clone(), out))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        book.for_each_text_mut(|path, content| {
            if let Some(output) = result.remove(path) {
                *content = output;
            }
        });

        if status.level() <= Level::WARN {
            warn!("Finished with problems");
        } else {
            info!("Finished");
        }

        Ok(book)
    }
}

impl Environment {
    fn resolve(&self, content: &mut Pages<'_>) {
        self.validate();

        let page_paths = &content.paths(&self.root_dir);

        let ticker = ticker!(Level::INFO, "process", "processing links").entered();

        for (base, link) in content.links_mut() {
            let file_url = match if let Some(link) = link.link.strip_prefix('/') {
                self.vcs.root.join(link)
            } else {
                base.join(&link.link)
            } {
                Ok(url) => url,
                Err(e) => {
                    debug!("ignoring unparsable link {:?}: {e}", &*link.link);
                    link.status = LinkStatus::Ignored;
                    continue;
                }
            };

            let env = self;
            let page_url = base.as_ref();

            if let Some(book) = &env.config.book_url
                && let Some(path) = book.0.make_relative(&file_url)
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
                let (_, url_suffix) = UrlSuffix::take(file_url);
                let dest = ResolveFile {
                    hint,
                    url_suffix,
                    is_vcs: true,
                    file_url: url,
                    page_url,
                    page_paths,
                    link,
                };
                let _span = dest.span(&ticker);
                self.resolve_file(dest);
            } else if file_url.scheme() == "file" {
                let (file_url, url_suffix) = UrlSuffix::take(file_url);
                let dest = ResolveFile {
                    hint: link.hint,
                    url_suffix,
                    is_vcs: false,
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
            is_vcs,
            link,
        }: ResolveFile,
    ) {
        if url_suffix.query.is_some() || url_suffix.fragment.is_some() {
            trace!(?url_suffix);
        }

        let relative_to_repo = match self.vcs.try_file(&file_url) {
            Ok(path) => path,
            Err(err) => {
                link.unreachable(vec![(file_url, err)]);
                return;
            }
        };

        let relative_to_book = self
            .root_dir
            .make_relative(&file_url)
            .expect("should be a file");

        trace!(?relative_to_book);

        let should_link = is_vcs.tap(|r| trace!(is_vcs = r))
            || relative_to_book
                .starts_with("../")
                .tap(|r| trace!(not_in_book = r))
            || (relative_to_book.ends_with(".md") && !page_paths.contains(&relative_to_book))
                .tap(|r| trace!(book_assets = r))
            || (self.config.always_link.iter())
                .any(|suffix| file_url.path().ends_with(suffix))
                .tap(|r| trace!(always_link = r));

        trace!(?should_link);

        if !should_link {
            if link.link.starts_with('/') {
                // mdBook doesn't support absolute paths like VS Code does
                let rewritten = page_url
                    .make_relative(&url_suffix.restored(file_url))
                    .expect("both should be file: urls");
                link.rewritten(rewritten);
            } else {
                link.unchanged();
            }
        } else {
            match self.vcs.link.to_link(&relative_to_repo.path, hint) {
                Ok(href) => {
                    link.permalink(url_suffix.restored(href).as_str().to_owned());
                }
                Err(err) => {
                    link.status = LinkStatus::Error(format!("{err}"));
                    debug!(status = ?link.status, link = ?&*link.link);
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

        let try_pages = {
            let path = path
                .strip_suffix(".html")
                .inspect(|_| trace!("removing .html suffix"))
                .unwrap_or(&path);
            // one does not simply avoid trailing slash issues...
            // https://github.com/slorber/trailing-slash-guide
            if is_index {
                &[
                    // enforce that index.html pages should consistently
                    // be addressed with a trailing slash
                    format!("{path}index.md"),
                    format!("{path}README.md"),
                ] as &[_]
            } else {
                &[
                    format!("{path}.md"),
                    // all major hosting providers implicitly redirect
                    // /folder to /folder/, so these are okay
                    format!("{path}/index.md"),
                    format!("{path}/README.md"),
                ]
            }
        };

        for page in try_pages {
            trace!("trying book page {page:?}");

            let file_url = (self.root_dir)
                .join(page)
                .expect("should be a valid url")
                .tap_mut(|u| u.set_query(file_url.query()))
                .tap_mut(|u| u.set_fragment(file_url.fragment()));

            if page_paths.contains(page) {
                let rewritten = page_url
                    .make_relative(&file_url)
                    .expect("both should be file: urls");
                link.rewritten(rewritten);
                return;
            }

            trace!("not found: {file_url}");
            not_found.push((file_url, PathStatus::NotInBook));
        }

        if !is_index {
            let try_file = self.root_dir.join(&path).expect("should be a valid url");

            match self.vcs.try_file(&try_file) {
                Ok(result) if !result.metadata.is_dir() => {
                    let file_url = try_file
                        .tap_mut(|u| u.set_query(file_url.query()))
                        .tap_mut(|u| u.set_fragment(file_url.fragment()));

                    let rewritten = page_url
                        .make_relative(&file_url)
                        .expect("both should be file: urls");

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

    #[inline]
    fn validate(&self) {
        debug_assert!(
            self.root_dir.as_str().ends_with('/'),
            "book_src should have a trailing slash, got {}",
            self.root_dir
        );
        debug_assert!(
            self.vcs.root.as_str().ends_with('/'),
            "vcs_root should have a trailing slash, got {}",
            self.vcs.root
        );
    }

    fn new(book: &PreprocessorContext) -> Result<Result<Self>> {
        let config = (book.config)
            .preprocessor(&[PREPROCESSOR_NAME, "mdbook-link-forever"])
            .inspect(emit_debug!("{:#?}"))
            .context("Failed to read preprocessor config from book.toml")?;

        let vcs = match VersionControl::try_from_git(&config, &book.config) {
            Ok(Ok(vcs)) => vcs,
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };

        let markdown = book.config.markdown_options();

        let root_dir = (book.root)
            .canonicalize()
            .context("Failed to locate book root")?
            .join(&book.config.book.src)
            .to_directory_url();

        Ok(Ok(Self {
            vcs,
            root_dir,
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
    is_vcs: bool,
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
    fn span(&self, parent: impl Into<Option<tracing::Id>>) -> EnteredSpan {
        let Self {
            file_url,
            page_url,
            hint,
            link,
            ..
        } = self;
        if tracing::enabled!(Level::DEBUG) {
            ticker_item! {
                parent, Level::INFO, "file_link",
                %file_url, %page_url, ?hint,
                "{:?}", &*link.link
            }
        } else {
            ticker_item!(parent, Level::INFO, "file_link", "{:?}", &*link.link)
        }
        .entered()
    }
}

impl<'a, 'r> ResolveBook<'a, 'r> {
    #[inline]
    fn span(&self, parent: impl Into<Option<tracing::Id>>) -> EnteredSpan {
        let Self {
            file_url,
            page_url,
            path,
            link,
            ..
        } = self;
        if tracing::enabled!(Level::DEBUG) {
            ticker_item! {
                parent, Level::INFO, "book_link",
                %file_url, %page_url, ?path,
                "{:?}", &*link.link
            }
        } else {
            ticker_item!(parent, Level::INFO, "book_link", "{:?}", &*link.link)
        }
        .entered()
    }
}

/// Configuration for the preprocessor.
///
/// This is deserialized from book.toml.
///
/// Doc comments for attributes populate the `configuration.md` page in the docs.
#[derive(clap::Parser, Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Config {
    /// Use a custom link format for platforms other than GitHub.
    ///
    /// Should be a string that contains the following placeholders that will be
    /// filled in at build time:
    ///
    /// - `{ref}` — the Git reference (tag or commit ID) resolved at build time
    /// - `{path}` — path to the linked file relative to repo root, without a leading `/`
    ///
    /// For example, the following configures generated links to use GitLab's format:
    ///
    /// ```toml
    /// repo-url-template = "https://gitlab.haskell.org/ghc/ghc/-/tree/{ref}/{path}"
    /// ```
    ///
    /// Note that information such as repo owner or name will not be filled in. If URLs to
    /// your Git hosting service require such items, you should hard-code them in the pattern.
    #[serde(default)]
    #[arg(long, value_name("FORMAT"), verbatim_doc_comment)]
    repo_url_template: Option<String>,

    /// Specify the canonical URL at which you deploy your book.
    ///
    /// Should be a qualified URL. For example:
    ///
    /// ```toml
    /// book-url = "https://me.github.io/my-awesome-crate/"
    /// ```
    ///
    /// Enables validation of hard-coded links to book pages. The preprocessor will
    /// warn you about links that are no longer valid (file not found) at build time.
    ///
    /// This is mainly used with mdBook's `{{#include}}` feature, where sometimes you
    /// have to specify full URLs because path-based links are not supported.
    #[serde(default)]
    #[arg(long, value_name("URL"), verbatim_doc_comment)]
    book_url: Option<UrlPrefix>,

    /// Convert some paths to permalinks even if they are under `src/`.
    ///
    /// By default, links to files in your book's `src/` directory will not be transformed,
    /// since they are already copied to build output as static files. If you want such files
    /// to always be rendered as permalinks, specify their file extensions here.
    ///
    /// For example, to use permalinks for Rust source files even if they are in the book's
    /// `src/` directory:
    ///
    /// ```toml
    /// always-link = [".rs"]
    /// ```
    #[serde(default)]
    #[arg(
        long,
        value_delimiter(','),
        value_name("EXTENSIONS"),
        verbatim_doc_comment
    )]
    always_link: Vec<String>,

    /// Exit with a non-zero status code when there are warnings.
    ///
    /// Warnings are always printed to the console regardless of this option.
    #[serde(default)]
    #[arg(long, value_enum, value_name("MODE"), default_value_t = Default::default())]
    fail_on_warnings: OnWarning,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    after: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    before: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    renderers: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    command: Option<String>,
}

#[derive(Clone)]
struct UrlPrefix(Url);

impl From<Url> for UrlPrefix {
    fn from(mut url: Url) -> Self {
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Self(url)
    }
}

impl FromStr for UrlPrefix {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s.parse::<Url>()?))
    }
}

impl<'de> Deserialize<'de> for UrlPrefix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let url = Url::deserialize(deserializer)?;
        Ok(Self::from(url))
    }
}

impl Debug for UrlPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UrlPrefix")
            .field(&format_args!("\"{}\"", self.0))
            .finish()
    }
}

#[must_use]
#[derive(Debug)]
struct UrlSuffix {
    query: Option<String>,
    fragment: Option<String>,
}

impl UrlSuffix {
    fn take(mut url: Url) -> (Url, Self) {
        let query = url.query().map(|s| s.to_owned());
        let fragment = url.fragment().map(|s| s.to_owned());
        url.set_query(None);
        url.set_fragment(None);
        (url, Self { query, fragment })
    }

    fn restored(self, mut url: Url) -> Url {
        let Self { query, fragment } = self;

        match (url.query(), &query) {
            (Some(_), None) => {}
            _ => url.set_query(query.as_deref()),
        }

        match (url.fragment(), &fragment) {
            (Some(_), None) => {}
            _ => url.set_fragment(fragment.as_deref()),
        }

        url
    }
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            vcs,
            root_dir,
            markdown,
            config: _,
        } = self;
        f.debug_struct("Environment")
            .field("root_dir", &format_args!("\"{root_dir}\""))
            .field("vcs", &vcs)
            .field("markdown", &markdown)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for VersionControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            root,
            link,
            repo: _,
        } = self;
        f.debug_struct("VersionControl")
            .field("root", &format_args!("\"{root}\""))
            .field("link", &link)
            .finish_non_exhaustive()
    }
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
