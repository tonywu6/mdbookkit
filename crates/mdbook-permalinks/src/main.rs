#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![allow(clippy::result_large_err)]

use std::{collections::HashMap, convert::Infallible, ffi::OsStr, fmt::Debug, path::Path};

use anyhow::{Context, Result};
use data_encoding::BASE64;
use mdbook_markdown::pulldown_cmark::{Event, Parser};
use mdbook_preprocessor::{PreprocessorContext, book::Book};
use tap::Tap;
use tracing::{Level, debug, error_span, info, instrument, trace, warn};
use url::Url;

use mdbookkit::{
    book::{PreprocessorHelper, book_from_stdin, should_emit_issues},
    config::{BaseDir, validate_config_examples},
    diagnostics::{IssueReporter, SourceCode},
    emit, emit_debug, emit_error,
    env::is_logging,
    error::{ProgramExit, Show, WithDebugContext, has_severity},
    level_enabled,
    logging::init_logging,
    markdown::patch_stream,
    plural, ticker, ticker_item,
    url::{RelativeUrl, UrlUtil},
};

use self::{
    diagnostics::link_issue,
    link::{
        BookPathError, ContentInterest, Link, LinkError, LinkHelp, LinkReader, LinkState, PathError,
    },
    options::{Config, DevModeConfig, Options},
    vcs::{GitIgnore, RepoPath, TryRepoPath, VersionControl},
};

mod diagnostics;
mod link;
mod options;
mod vcs;

fn main() {
    init_logging();
    let _span = error_span!({ PREPROCESSOR_NAME }).entered();
    let Program { command } = clap::Parser::parse();
    match command {
        Some(Command::Supports { .. }) => Ok(()),
        Some(Command::ValidateConfig) => {
            validate_config_examples::<Config>().or_else(emit_error!())
        }
        None => mdbook(),
    }
    .exit()
}

fn mdbook() -> Result<(), ()> {
    let (ctx, mut book) = book_from_stdin()
        .context("failed to read from mdBook")
        .or_else(emit_error!())?;

    let env = match Environment::new(&ctx, &book) {
        Ok(Ok(env)) => env,
        Ok(Err(err)) => {
            warn!("{:?}", err.context("preprocessor will be disabled"));
            ctx.print(book).or_else(emit_error!())?;
            return Ok(());
        }
        Err(err) => Err(err)
            .context("could not initialize the preprocessor")
            .or_else(emit_error!())?,
    };

    env.resolve(&ctx, &mut book)?;

    (env.options.fail_on_warnings)
        .check()
        .or_else(emit_error!())?;

    ctx.print(book).or_else(emit_error!())?;

    if has_severity(Level::WARN) {
        warn!("finished with warnings");
    } else {
        info!("finished");
    }

    Ok(())
}

struct Environment {
    repo: VersionControl,
    book: BookLayout,
    site_url: BaseDir,
    options: Options,
}

impl Environment {
    fn new(ctx: &PreprocessorContext, book: &Book) -> Result<Result<Self>> {
        let config = Config::new(ctx)?;
        debug!("{config:#?}");

        let repo = match VersionControl::try_from_git(&config, &ctx.root) {
            Ok(Ok(repo)) => repo,
            Ok(Err(err)) => return Ok(Err(err.context(GIT_ERROR))),
            Err(err) => return Err(err.context(GIT_ERROR)),
        };
        static GIT_ERROR: &str =
            "could not obtain the necessary git information to generate permalinks";

        let book = BookLayout::new(ctx, book, &repo)?;

        let Config {
            repo_url: _,
            site_url,
            mut options,
        } = config;

        let site_url = site_url
            .or_else(|| options.site_url.take())
            .unwrap_or_default()
            .resolve(&book.base_dir.path);

        Ok(Ok(Self {
            repo,
            book,
            site_url,
            options,
        }))
    }

    fn resolve(&self, ctx: &PreprocessorContext, book: &mut Book) -> Result<(), ()> {
        let progress = ticker!(Level::INFO, "process", "processing links").entered();

        let markdown = ctx.markdown_options();
        let repo_url = self.repo.root();

        let mut outputs = HashMap::new();
        let mut reports = Vec::new();
        let mut stats = Statistics::default();

        ctx.for_each_page(book, |page_url, source| {
            let mut reader = LinkReader::new(source);

            let mut report = IssueReporter {
                issues: vec![],
                source: SourceCode {
                    source_path: repo_url.as_base().show_path(&page_url).to_string().into(),
                    source_code: source,
                },
            };

            let stream = Parser::new_ext(source, markdown)
                .into_offset_iter()
                .map(Some)
                .chain(std::iter::once(None)) // EOF
                .flat_map(|event| {
                    trace! { "{:?}", std::fmt::from_fn(|f| if let Some((event, span)) = &event {
                        write!(f, "{span:?} {event:?}")
                    } else {
                        write!(f, "(EOF)")
                    }) };
                    reader
                        .read(event)
                        .with_debug(&page_url, "file")
                        .context("failed to parse file as markdown")
                        .or_else(emit_error!())
                        .ok()
                })
                .flatten()
                .map(|patch| {
                    let mut links = match patch {
                        Patch::Link(links) => links,
                        Patch::Skip(chunk) => return (Patch::Skip(chunk), None),
                        Patch::SkipOne(elem) => return (Patch::SkipOne(elem), None),
                    };

                    for link in links.links_mut() {
                        let Some((resolver, link_url)) = self.triage(&page_url, link) else {
                            continue;
                        };

                        let _span = if !is_logging() {
                            ticker_item!(&progress, Level::INFO, "resolve", "{:?}", link.href())
                        } else if level_enabled!(Level::TRACE) {
                            ticker_item! {
                                &progress, Level::TRACE, "resolve",
                                interest = ?resolver.interest,
                                location = ?resolver.location,
                                page = ?repo_url.as_base().show_path(&page_url),
                                link = ?link.href(),
                                url  = ?repo_url.as_base().show_path(&link_url),
                            }
                        } else {
                            ticker_item!(&progress, Level::DEBUG, "resolve", "{:?}", link.href())
                        }
                        .entered();

                        resolver.resolve(link_url, link);

                        report.issues.extend(link_issue(repo_url, &page_url, link));

                        stats.count(link);
                    }

                    links.emit()
                });

            let output = patch_stream(source, stream)
                .with_debug(&page_url, "file")
                .context("error generating output for file")
                .or_else(emit_error!())?;

            outputs.insert(page_url, output);
            reports.push(report);

            Ok(())
        })?;

        if should_emit_issues(ctx) {
            for report in IssueReporter::sorted(reports) {
                report.emit(emit!());
            }
        }

        ctx.for_each_page_mut(book, |path, content| {
            *content = outputs
                .remove(&path)
                .with_debug(&path, "file")
                .expect("`outputs` should contain path");
            Ok(())
        })?;

        stats.print();

        Ok(())
    }

    fn triage<'a>(&'a self, page_url: &'a Url, link: &Link<'_>) -> Option<(Resolver<'a>, Url)> {
        let link_url = match match link.repo_relative() {
            Some(href) => self.repo.root().join(href),
            None => page_url.join(link.href()),
        } {
            Ok(url) => url,
            Err(e) => {
                trace!("ignoring unparsable link {:?}: {e}", link.href());
                return None;
            }
        };

        if let Some(site) = &self.site_url.http
            && let Some(href) = site.as_base().make_relative_scoped(&link_url)
        {
            let link_url = (self.book.base_dir.as_file_url().as_base()).make_absolute(&href);
            let resolver = Resolver {
                env: self,
                page_url,
                location: ContentLocation::Book,
                interest: link.interest(),
            };
            Some((resolver, link_url))
        } else if let Some((href, interest)) = self.repo.scheme().extract(&link_url) {
            let link_url = (self.repo.root().as_base()).make_absolute(&href);
            let resolver = Resolver {
                env: self,
                page_url,
                location: ContentLocation::Repo,
                interest,
            };
            Some((resolver, link_url))
        } else if link_url.scheme() == "file" {
            let location = if (self.options.always_link.iter())
                .any(|suffix| link_url.path().ends_with(suffix))
            {
                ContentLocation::Repo
            } else {
                ContentLocation::Any
            };
            let resolver = Resolver {
                env: self,
                page_url,
                location,
                interest: link.interest(),
            };
            Some((resolver, link_url))
        } else {
            None
        }
    }
}

struct Resolver<'a> {
    env: &'a Environment,
    page_url: &'a Url,
    location: ContentLocation,
    interest: ContentInterest,
}

#[derive(Debug, Clone, Copy)]
enum ContentLocation {
    Book,
    Repo,
    Any,
}

#[derive(Debug)]
enum LinkResult {
    RepoLink { path: RepoPath },
    BookResource { path: RepoPath },
    MarkdownPage { file: Url },
}

impl Resolver<'_> {
    fn resolve(&self, link_url: Url, link: &mut Link<'_>) {
        use {BookPathError::*, ContentLocation::*, LinkResult::*, PathError::*};

        if link.repo_relative().is_some() && link_url.path() == self.env.repo.root().path() {
            self.ambiguous_link_to_root(link_url, link);
            return;
        }

        let orig_url = link_url.clone();

        match match match self.location {
            Any | Repo => self.try_link(link_url),
            Book => self.try_derived_links(link_url),
        } {
            Ok(link) => match self.location {
                Any | Repo => Ok(link),
                Book => match link {
                    link @ RepoLink { .. } => Ok(link),
                    link @ BookResource { .. } => Ok(link),
                    MarkdownPage { file } => {
                        if orig_url.path().ends_with(".md") {
                            debug!("unexpected `.md` extension in link");
                            Err(NoSuchPage(UnexpectedFileExtension).at(file))
                        } else {
                            Ok(MarkdownPage { file })
                        }
                    }
                },
            },

            Err(err) => match self.location {
                Any => match &err.error {
                    NotFound if self.is_in_book(&err.cause) => self.try_derived_links(err.cause),
                    NoSuchPage(DirectoryHasNoIndexFile) => {
                        match self.try_derived_links(err.cause) {
                            Ok(path) => Ok(path),
                            Err(mut err)
                                if matches!(err.error, NoSuchPage(NoResourceAtLocation(..))) =>
                            {
                                err.error = NoSuchPage(DirectoryHasNoIndexFile);
                                Err(err)
                            }
                            Err(err) => Err(err),
                        }
                    }
                    _ => Err(err),
                },
                Repo | Book => Err(err),
            },
        } {
            Ok(result) => self.write_link(result, link),

            Err(mut e) => {
                match e.error {
                    NotFound if matches!(self.location, Any) => {
                        e.help = (self.try_find_other(&self.env.site_url, &orig_url))
                            .or_else(|| self.try_find_other(&self.env.book.base_dir, &orig_url));
                    }

                    NotADirectory => {
                        if let Ok(edited) = self
                            .edit_link(link, |url| {
                                url.ensure_no_trailing_slash();
                                Ok(())
                            })
                            .context("could not correct the link")
                            .or_else(emit_debug!())
                        {
                            e.help = Some(LinkHelp::GenericEdit {
                                help: "try removing the trailing slash",
                                edited,
                            })
                        }
                    }

                    NoSuchPage(UnexpectedFileExtension) => {
                        if let Ok(edited) = self
                            .edit_link(link, |url| {
                                if let Some(path) = url.path().strip_suffix(".md") {
                                    #[allow(clippy::unnecessary_to_owned)]
                                    url.set_path(&path.to_owned());
                                }
                                Ok(())
                            })
                            .context("could not correct the link")
                            .or_else(emit_debug!())
                        {
                            e.help = Some(LinkHelp::GenericEdit {
                                help: "try removing the extension",
                                edited,
                            })
                        }
                    }

                    _ => {}
                }
                link.error(e)
            }
        };
    }

    fn is_in_book(&self, url: &Url) -> bool {
        (self.env.book.base_dir)
            .as_file_url()
            .as_base()
            .make_relative_scoped(url)
            .is_some()
    }

    #[instrument(level = "debug", skip_all)]
    fn try_link(&self, url: Url) -> Result<LinkResult, LinkError> {
        use {ContentLocation::*, LinkResult::*};

        match self.env.repo.try_file(url)? {
            TryRepoPath::Canonical { link } => self.try_link_in_repo(link),

            TryRepoPath::Noncanonical { link, real } => match self.location {
                Repo => self.try_link_in_repo(real),

                Book | Any => {
                    debug!("could be a symlink? trying the verbatim path");
                    let link = self.try_link_in_book(link)?;
                    match link {
                        BookResource { .. } | MarkdownPage { .. } => {
                            debug!("path is in the book and available in output");
                            Ok(link)
                        }
                        RepoLink { .. } => {
                            debug!("path is outside the book; trying the canonical path");
                            self.try_link_in_repo(real)
                        }
                    }
                }
            },
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn try_link_in_repo(&self, path: RepoPath) -> Result<LinkResult, LinkError> {
        use {ContentLocation::*, GitIgnore::*, LinkResult::*, PathError::*};

        let is_ignored = path.is_ignored;
        match self.location {
            Book | Any => match (self.try_link_in_book(path)?, is_ignored) {
                (link @ BookResource { .. }, ..) => Ok(link),
                (link @ MarkdownPage { .. }, ..) => Ok(link),
                (link @ RepoLink { .. }, NotIgnored) => Ok(link),
                (RepoLink { path, .. }, Ignored) => Err(GitIgnored.at(path.url)),
            },

            Repo => match is_ignored {
                NotIgnored => Ok(RepoLink { path }),
                Ignored => Err(GitIgnored.at(path.url)),
            },
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn try_link_in_book(&self, path: RepoPath) -> Result<LinkResult, LinkError> {
        use {BookPathError::*, LinkResult::*, TryBookPath::*};

        match self.env.book.try_file(&path.relative) {
            Some(SourcePath { resolved } | PublicPath { resolved }) => {
                if path.is_dir {
                    trace!("directory exists and has an index file");
                }
                Ok(MarkdownPage { file: resolved })
            }

            Some(NoSuchPage) => {
                if path.is_dir {
                    debug!("directory exists but has no index file");
                    Err(PathError::NoSuchPage(DirectoryHasNoIndexFile).at(path.url))
                } else if path.std_path.extension() == Some(OsStr::new("md")) {
                    debug!("markdown file not in SUMMARY.md");
                    Err(PathError::NoSuchPage(MarkdownFileNotIncluded).at(path.url))
                } else {
                    debug!("path is a static file to be copied to output");
                    Ok(BookResource { path })
                }
            }

            None => Ok(RepoLink { path }),
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn try_derived_links(&self, url: Url) -> Result<LinkResult, LinkError> {
        use {BookPathError::*, PathError::*};

        let mut errors = vec![];

        for url in BookLayout::source_paths_for(&url) {
            trace! {
                "trying derived path {:?}",
                self.env.repo.root().as_base().show_path(&url)
            };

            match self.try_link(url) {
                Ok(path) => return Ok(path),
                Err(err) => match &err.error {
                    NotFound => errors.push(err),
                    NotADirectory => errors.push(err),
                    // probing `path/index.md` (derived from `path`) could
                    // cause NotADirectory when `path` is actually a file
                    _ => return Err(err),
                },
            }
        }

        Err(NoSuchPage(NoResourceAtLocation(errors)).at(url))
    }

    fn write_link(&self, result: LinkResult, link: &mut Link<'_>) {
        use LinkResult::*;

        if let RepoLink { path } = result {
            let href = if let Some(dev) = &*self.env.options.dev_mode {
                if let (ContentInterest::Raw, false) = (self.interest, path.is_dir) {
                    match dev.to_embed_link(&path.std_path) {
                        Ok(Some(href)) => {
                            trace!("rewriting to data uri");
                            Some(href)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            let err = err.at(path.url);
                            link.error(err);
                            return;
                        }
                    }
                } else {
                    let href = dev.to_editor_uri(&path.url);
                    trace!("rewriting to editor uri: {:?}", href.show());
                    Some(href.into())
                }
            } else {
                None
            };

            if let Some(href) = href {
                link.permalink(href);
            } else {
                let href = (self.env.repo.scheme()).to_link(&path.relative, self.interest);
                trace!("rewriting to permalink: {:?}", href.show());
                link.permalink(href.into());
            };
        } else {
            let file = match result {
                BookResource { path } => path.url,
                MarkdownPage { mut file } => {
                    if self.interest == ContentInterest::Raw {
                        file.replace_suffix(".md", ".html");
                    }
                    file
                }
                RepoLink { .. } => unreachable!(),
            };

            let href = (self.page_url.as_base())
                .make_relative(&file)
                .expect("both should be file urls");

            if self.env.options.qualify_book_links.0
                && let Some(base) = self.env.site_url.as_http_url()
            {
                let mut href = base.as_base().make_absolute(&href);
                href.replace_suffix(".md", ".html");
                link.permalink(href.into());
            } else if href != link.href() {
                trace!("rewriting to book link: {:?}", href.show_path());
                link.book_link(href);
            } else {
                trace!("keeping the link as-is");
                link.no_change();
            }
        }
    }

    fn ambiguous_link_to_root(&self, link_url: Url, link: &mut Link<'_>) {
        let Environment { repo, .. } = self.env;

        let href = (repo.root().as_base().make_relative_scoped(&link_url))
            .expect("`link_url` should be the same as `repo.root`");
        debug_assert_eq!(href.encoded_path(), "");

        let to_repo = repo.scheme().to_link_at_head(&href, link.interest()).into();

        let book_url = (self.env.book.base_dir.file)
            .clone()
            .include_after_path(&link_url);
        let to_book = (repo.root().as_base().make_relative_scoped(&book_url))
            .expect("`book_root` should be under `repo.root`")
            .into_absolute_path();
        let (to_book, to_book_relative) = if to_book == link.href() {
            let relative = (self.page_url.as_base())
                .make_relative(&book_url)
                .expect("both should be file urls");
            (relative, true)
        } else {
            (to_book, false)
        };
        let to_book = to_book.consume_with(<_>::into);

        link.error(LinkError {
            error: PathError::AmbiguousLinkToRoot,
            cause: link_url,
            help: Some(LinkHelp::LinkToRoot {
                to_repo,
                to_book,
                to_book_relative,
            }),
        });
    }

    fn try_find_other(&self, base: &BaseDir, url: &Url) -> Option<LinkHelp> {
        use LinkResult::*;

        let alternative = base.transplant(url).located_in(self.env.repo.root())?;

        let (url, relative) = match self.try_derived_links(alternative).ok()? {
            RepoLink {
                path: RepoPath { url, relative, .. },
            } => (url, relative),
            BookResource {
                path: RepoPath { url, relative, .. },
            } => (url, relative),
            MarkdownPage { file, .. } => {
                let relative = self.env.repo.root().as_base().make_relative_scoped(&file)?;
                (file, relative)
            }
        };

        Some(LinkHelp::FoundOther {
            from_page: self.page_url.as_base().make_relative(&url)?,
            from_repo: relative.into_absolute_path(),
        })
    }

    fn edit_link<F>(&self, link: &Link<'_>, edit: F) -> Result<String>
    where
        F: FnOnce(&mut Url) -> Result<()>,
    {
        if let Ok(mut url) = link.href().parse::<Url>() {
            edit(&mut url)?;
            Ok(url.into())
        } else if let Some(link) = link.repo_relative() {
            let mut url = self.env.repo.root().join(link)?;
            edit(&mut url)?;
            let url = (self.env.repo.root().as_base().make_relative(&url))
                .context("could not restore relative url")?;
            Ok(url.into_absolute_path().consume_with(<_>::into))
        } else {
            let mut url = self.page_url.join(link.href())?;
            edit(&mut url)?;
            let url = (self.page_url.as_base().make_relative(&url))
                .context("could not restore relative url")?;
            Ok(url.consume_with(<_>::into))
        }
    }
}

struct BookLayout {
    base_dir: BaseDir,
    base_url: RelativeUrl,
    source_paths: HashMap<String, Url>,
    public_paths: HashMap<String, Url>,
}

impl BookLayout {
    fn new(ctx: &PreprocessorContext, book: &Book, vcs: &VersionControl) -> Result<Self> {
        let mut source_paths = HashMap::new();
        let mut public_paths = HashMap::new();

        let vcs_root = vcs.root().as_base();

        ctx.for_each_page(book, |url, _| -> Result<_, Infallible> {
            if (url.path().ends_with("/index.md") || url.path().ends_with("/README.md"))
                && let Ok(mut path) = url.join(".")
            {
                path.ensure_trailing_slash();
                if let Some(href) = vcs_root.make_relative(&path) {
                    let href = href.encoded_path().to_owned();
                    public_paths.insert(href, url.clone());
                }
                path.ensure_no_trailing_slash();
                if let Some(href) = vcs_root.make_relative(&path) {
                    let href = href.encoded_path().to_owned();
                    public_paths.insert(href, url.clone());
                }
            }

            if let Some(href) = vcs_root.make_relative(&url) {
                let href = href.encoded_path().to_owned();
                if let Some(href) = href.strip_suffix(".md") {
                    public_paths.insert(format!("{href}.html"), url.clone());
                    public_paths.insert(href.to_owned(), url.clone());
                }

                source_paths.insert(href, url.clone());
            }

            Ok(())
        });

        let base_dir = BaseDir::new(ctx.page_dir()?)?;

        let base_url = vcs_root
            .make_relative(&base_dir.file)
            .with_debug(&vcs_root, "repo")
            .with_debug(&*base_dir.path, "book")
            .context("book is outside of repo")?;

        Ok(Self {
            base_dir,
            base_url,
            source_paths,
            public_paths,
        })
    }

    #[instrument(level = "trace", "book_try_file", skip_all, fields(path = ?url.show_path()))]
    fn try_file(&self, url: &RelativeUrl) -> Option<TryBookPath> {
        let root = self.base_url.encoded_path();
        let path = url.encoded_path();
        if let Some(canonical) = self.source_paths.get(path) {
            trace!("source path to {:?}", canonical.show());
            let resolved = canonical.clone().include_after_path(url);
            Some(TryBookPath::SourcePath { resolved })
        } else if let Some(canonical) = self.public_paths.get(path) {
            trace!("public path to {:?}", canonical.show());
            let resolved = canonical.clone().include_after_path(url);
            Some(TryBookPath::PublicPath { resolved })
        } else if path.starts_with(root) || root.strip_prefix(path) == Some("/") {
            debug!("no matching source file");
            Some(TryBookPath::NoSuchPage)
        } else {
            trace!("outside the book");
            None
        }
    }

    fn source_paths_for(url: &Url) -> Vec<Url> {
        if url.path().ends_with('/') {
            vec![
                (url.clone()).tap_mut(|u| u.set_path(&format!("{}index.md", u.path()))),
                (url.clone()).tap_mut(|u| u.set_path(&format!("{}README.md", u.path()))),
            ]
        } else if let Some(path) = url.path().strip_suffix(".html") {
            vec![
                (url.clone()).tap_mut(|u| u.set_path(&format!("{path}.md"))),
                (url.clone()),
            ]
        } else {
            let mut paths = vec![
                (url.clone()).tap_mut(|u| u.set_path(&format!("{}.md", url.path()))),
                (url.clone()).tap_mut(|u| u.set_path(&format!("{}/index.md", url.path()))),
                (url.clone()).tap_mut(|u| u.set_path(&format!("{}/README.md", url.path()))),
                (url.clone()),
            ];
            if let Some(mut path) = url.path_segments()
                && let Some(name) = path.next_back()
                && name.contains('.')
            {
                paths.swap(0, 3);
            }
            paths
        }
    }
}

#[derive(Debug, Clone)]
enum TryBookPath {
    NoSuchPage,
    SourcePath { resolved: Url },
    PublicPath { resolved: Url },
}

impl Debug for BookLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookLayout")
            .field(
                "source_paths",
                &std::fmt::from_fn(|f| f.debug_set().entries(self.source_paths.keys()).finish()),
            )
            .field(
                "public_paths",
                &std::fmt::from_fn(|f| f.debug_set().entries(self.public_paths.keys()).finish()),
            )
            .finish_non_exhaustive()
    }
}

enum Patch<'a, E> {
    Link(E),
    Skip(std::vec::IntoIter<Event<'a>>),
    SkipOne(std::iter::Once<Event<'a>>),
}

impl<'a, E: Iterator<Item = Event<'a>>> Iterator for Patch<'a, E> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Link(events) => events.next(),
            Self::Skip(events) => events.next(),
            Self::SkipOne(event) => event.next(),
        }
    }
}

#[derive(Default)]
struct Statistics {
    ignored: usize,
    unchanged: usize,
    rewritten: usize,
    permalink: usize,
    error: usize,
    total: usize,
}

impl Statistics {
    fn count(&mut self, link: &Link<'_>) {
        self.total += 1;
        match link.state() {
            Ok(LinkState::Unsupported) => self.ignored += 1,
            Ok(LinkState::BookLinkChecked) => self.unchanged += 1,
            Ok(LinkState::BookLinkUpdated) => self.rewritten += 1,
            Ok(LinkState::Permalink) => self.permalink += 1,
            Err(..) => self.error += 1,
        }
    }

    fn print(&self) {
        let Self {
            ignored,
            unchanged,
            rewritten,
            permalink,
            error,
            total,
        } = self;
        info!(
            "processed {total}: {permalink} to repo; {rewritten} to book; {error}; {unchanged}",
            total = plural!(total, "link"),
            permalink = plural!(permalink, "link"),
            rewritten = plural!(rewritten, "link"),
            error = plural!(error, "has error", "have errors"),
            unchanged = plural!(unchanged + ignored, "unchanged", "unchanged"),
        );
    }
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

impl DevModeConfig {
    fn to_embed_link(&self, path: &Path) -> Result<Option<String>, PathError> {
        if self.embed_images == Some(false) {
            return Ok(None);
        }
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(err) => return Err(PathError::from_io(err)),
        };
        static PREFIX: &str = "data:application/octet-stream;base64,";
        let encoding = BASE64;
        let mut href = String::with_capacity(PREFIX.len() + encoding.encode_len(data.len()));
        href.push_str(PREFIX);
        encoding.encode_append(&data, &mut href);
        debug_assert!(matches!(href.parse::<Url>(), Ok(..)));
        Ok(Some(href))
    }

    fn to_editor_uri(&self, file_url: &Url) -> Url {
        self.editor_uri.pattern_fill(|group| match group {
            "path" => {
                let path = file_url.path();
                let path = path.strip_prefix('/').unwrap_or(path);
                Some(path.into())
            }
            "url" => Some(file_url.as_str().into()),
            _ => None,
        })
    }
}

#[macro_export]
macro_rules! PREPROCESSOR_NAME {
    () => {
        env!("CARGO_PKG_NAME")
    };
}

static PREPROCESSOR_NAME: &str = PREPROCESSOR_NAME!();
