use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    str::FromStr,
};

use anyhow::{Context, Result, anyhow};
use git2::Repository;
use mdbook_markdown::pulldown_cmark;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext, book::Book};
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible};
use tracing::{level_filters::LevelFilter, warn};
use url::Url;

use mdbookkit::{
    book::{BookConfigHelper, BookHelper, book_from_stdin},
    diagnostics::Issue,
    emit_debug, emit_warning,
    error::OnWarning,
    logging::Logging,
};

use self::{
    link::{LinkStatus, PathStatus, RelativeLink},
    page::Pages,
    vcs::{Permalink, PermalinkFormat},
};

mod diagnostic;
mod link;
mod page;
#[cfg(test)]
mod tests;
mod vcs;

struct Permalinks;

impl Preprocessor for Permalinks {
    fn name(&self) -> &str {
        env!("CARGO_PKG_NAME")
    }

    fn run(&self, ctx: &PreprocessorContext, book: Book) -> Result<Book> {
        match Environment::new(ctx) {
            Ok(Ok(env)) => env.run(ctx, book),
            Ok(Err(err)) => {
                warn!("{:?}", err.context("preprocessor will be disabled"));
                Ok(book)
            }
            Err(err) => Err(err).context(format!(
                "failed to initialize preprocessor `{}`",
                self.name()
            )),
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
            let path = path.to_string_lossy();
            let url = self
                .root_dir
                .join(&path)
                .context("could not read path as a url")?;
            content
                .insert(url, &ch.content)
                .with_context(|| format!("failed to parse {path}"))?;
        }

        self.resolve(&mut content);

        let mut result = book
            .iter_chapters()
            .filter_map(|(path, _)| {
                let url = self.root_dir.join(&path.to_string_lossy()).ok()?;
                content
                    .emit(&url)
                    .tap_err(emit_warning!())
                    .ok()
                    .map(|output| (path.clone(), output.to_string()))
            })
            .collect::<HashMap<_, _>>();

        let status = self
            .report_issues(&content, |_| true)
            .names(|url| self.rel_path(url))
            .level(LevelFilter::WARN)
            .build()
            .to_stderr()
            .to_status();

        book.for_each_text_mut(|path, content| {
            if let Some(output) = result.remove(path) {
                *content = output;
            }
        });

        self.config.fail_on_warnings.check(status.level())?;

        Ok(book)
    }
}

impl Environment {
    fn resolve(&self, content: &mut Pages<'_>) {
        self.validate();

        let book_pages = &content.paths(&self.root_dir);

        for (base, link) in content.links_mut() {
            let file = if let Some(link) = link.link.strip_prefix('/') {
                self.vcs.root.join(link)
            } else {
                base.join(&link.link)
            }
            .context("could not derive url")
            .tap_err(emit_debug!());

            let Ok(file_url) = file else {
                link.status = LinkStatus::Ignored;
                continue;
            };

            let env = self;
            let page_url = base.as_ref();

            Resolver {
                link,
                page_url,
                file_url,
                book_pages,
                env,
            }
            .resolve();
        }
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
        let config = book
            .config
            .preprocessor(&[PREPROCESSOR_NAME, "mdbook-link-forever"])?;

        let vcs = match VersionControl::try_from_git(&config, &book.config) {
            Ok(Ok(vcs)) => vcs,
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };

        let markdown = book.config.markdown_options();

        let root_dir = book
            .root
            .canonicalize()
            .context("failed to locate book root")?
            .join(&book.config.book.src)
            .pipe(Url::from_directory_path)
            .map_err(|_| anyhow!("book `src` should be a valid absolute path"))?;

        Ok(Ok(Self {
            vcs,
            root_dir,
            markdown,
            config,
        }))
    }
}

#[must_use]
struct Resolver<'a, 'r> {
    link: &'a mut RelativeLink<'r>,
    file_url: Url,
    page_url: &'a Url,
    book_pages: &'a HashSet<String>,
    env: &'a Environment,
}

impl Resolver<'_, '_> {
    fn resolve(self) {
        if let Some(book) = &self.env.config.book_url
            && let Some(path) = book.0.make_relative(&self.file_url)
            && !path.starts_with("../")
        {
            self.resolve_book(path)
        } else {
            self.resolve_file()
        }
    }

    fn resolve_file(self) {
        let Self {
            link,
            page_url,
            file_url,
            env,
            ..
        } = self;

        let (file_url, hint, suffix, is_vcs) = if let Some((path, hint)) =
            env.vcs.link.to_path(&file_url)
            && let Ok(url) = env.vcs.root.join(&path)
        {
            let (_, suffix) = UrlSuffix::take(file_url);
            (url, hint, suffix, true)
        } else if file_url.scheme() == "file" {
            let (url, suffix) = UrlSuffix::take(file_url);
            (url, link.hint, suffix, false)
        } else {
            return;
        };

        let relative_to_repo = match self.env.vcs.try_file(&file_url) {
            Ok(path) => path,
            Err(err) => {
                link.status = LinkStatus::Unreachable(vec![(file_url, err)]);
                return;
            }
        };

        let relative_to_book = env
            .root_dir
            .make_relative(&file_url)
            .expect("should be a file");

        let should_link = is_vcs
            || relative_to_book.starts_with("../")
            || relative_to_book.ends_with(".md") && !self.book_pages.contains(&relative_to_book)
            || (env.config.always_link)
                .iter()
                .any(|suffix| file_url.path().ends_with(suffix));

        if !should_link {
            if link.link.starts_with('/') {
                // mdbook doesn't support absolute paths like VS Code does
                link.link = page_url
                    .make_relative(&suffix.restored(file_url))
                    .expect("both should be file: urls")
                    .into();
                link.status = LinkStatus::Rewritten;
            } else {
                link.status = LinkStatus::Unchanged;
            }
            return;
        }

        match env.vcs.link.to_link(&relative_to_repo.path, hint) {
            Ok(href) => {
                link.link = suffix.restored(href).as_str().to_owned().into();
                link.status = LinkStatus::Permalink;
            }
            Err(err) => link.status = LinkStatus::Error(format!("{err}")),
        }
    }

    /// Check hard-coded URLs to book content
    fn resolve_book(self, path: String) {
        let Self {
            file_url,
            page_url,
            link,
            ..
        } = self;

        let path = {
            let mut path = path;
            if let Some(idx) = path.find('#') {
                path.truncate(idx)
            };
            if let Some(idx) = path.find('?') {
                path.truncate(idx)
            };
            path
        };

        let mut not_found = vec![];

        let is_index = path.is_empty() || path.ends_with('/');

        let try_pages = {
            let path = path.strip_suffix(".html").unwrap_or(&path);
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
            let file_url = (self.env.root_dir)
                .join(page)
                .expect("should be a valid url")
                .tap_mut(|u| u.set_query(file_url.query()))
                .tap_mut(|u| u.set_fragment(file_url.fragment()));

            if self.book_pages.contains(page) {
                link.link = page_url
                    .make_relative(&file_url)
                    .expect("both should be file: urls")
                    .into();
                link.status = LinkStatus::Rewritten;
                return;
            }

            not_found.push((file_url, PathStatus::NotInBook));
        }

        if !is_index {
            let try_file = (self.env.root_dir)
                .join(&path)
                .expect("should be a valid url");

            match self.env.vcs.try_file(&try_file) {
                Ok(result) if !result.metadata.is_dir() => {
                    let file_url = try_file
                        .tap_mut(|u| u.set_query(file_url.query()))
                        .tap_mut(|u| u.set_fragment(file_url.fragment()));

                    link.link = page_url
                        .make_relative(&file_url)
                        .expect("both should be file: urls")
                        .into();
                    link.status = LinkStatus::Rewritten;

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

        link.status = LinkStatus::Unreachable(not_found);
    }
}

/// Configuration for the preprocessor.
///
/// This is deserialized from book.toml.
///
/// Doc comments for attributes populate the `configuration.md` page in the docs.
#[derive(clap::Parser, Deserialize, Default)]
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

#[derive(Debug, Clone)]
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

#[must_use]
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

fn main() -> Result<()> {
    Logging::default().init();
    let Program { command } = clap::Parser::parse();
    match command {
        None => {
            let (ctx, book) = book_from_stdin().context("failed to read from mdbook")?;
            Permalinks.run(&ctx, book)?.to_stdout(&ctx)?;
            Ok(())
        }
        Some(Command::Supports { .. }) => Ok(()),
        #[cfg(feature = "_testing")]
        Some(Command::Describe) => {
            print!("{}", mdbookkit::docs::describe_preprocessor::<Config>()?);
            Ok(())
        }
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
    #[cfg(feature = "_testing")]
    #[clap(hide = true)]
    Describe,
}

static PREPROCESSOR_NAME: &str = env!("CARGO_PKG_NAME");
