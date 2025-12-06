use std::{collections::HashMap, fmt::Debug, str::FromStr};

use anyhow::{Context, Result, anyhow};
use console::colors_enabled_stderr;
use log::LevelFilter;
use mdbook::preprocess::PreprocessorContext;
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible};
use url::Url;

use mdbookkit::{
    book::{
        book_from_stdin, book_into_stdout, config_from_book, for_each_chapter_mut, iter_chapters,
        smart_punctuation,
    },
    diagnostics::Issue,
    error::OnWarning,
    log_debug, log_warning,
    logging::{ConsoleLogger, is_logging},
    markdown::mdbook_markdown_options,
};

use self::{
    link::{LinkStatus, RelativeLink},
    page::{Fragments, Pages},
    vcs::{Permalink, PermalinkFormat},
};

mod diagnostic;
mod link;
mod page;
#[cfg(test)]
mod tests;
mod vcs;

fn main() -> Result<()> {
    ConsoleLogger::install(env!("CARGO_PKG_NAME"));

    let Program { command } = clap::Parser::parse();

    match command {
        Some(Command::Supports { .. }) => return Ok(()),
        None => {}
    }

    let (context, mut book) = book_from_stdin().context("failed to parse book content")?;

    let env = match Environment::new(&context) {
        Ok(Ok(env)) => env,
        Ok(Err(err)) => {
            log::warn!("{:?}", err.context("preprocessor will be disabled"));
            return book_into_stdout(&book);
        }
        Err(err) => {
            return Err(err).context(concat!(
                "failed to initialize preprocessor `",
                env!("CARGO_PKG_NAME"),
                "`"
            ));
        }
    };

    let mut content = Pages::new(env.markdown);

    for (path, ch) in iter_chapters(&book) {
        let url = env
            .book_src
            .join(&path.to_string_lossy())
            .context("could not read path as a url")?;
        content
            .insert(url, &ch.content)
            .with_context(|| path.display().to_string())
            .context("failed to parse Markdown source:")?;
    }

    env.resolve(&mut content);

    let mut result = iter_chapters(&book)
        .filter_map(|(path, _)| {
            let url = env.book_src.join(&path.to_string_lossy()).unwrap();
            content
                .emit(&url)
                .tap_err(log_warning!())
                .ok()
                .map(|output| (path.clone(), output.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let status = env
        .report_issues(&content, |_| true)
        .names(|url| env.rel_path(url))
        .level(LevelFilter::Warn)
        .logging(is_logging())
        .colored(colors_enabled_stderr())
        .build()
        .to_stderr()
        .to_status();

    for_each_chapter_mut(&mut book, |path, ch| {
        if let Some(output) = result.remove(&path) {
            ch.content = output
        }
    });

    book_into_stdout(&book)?;

    env.config.fail_on_warnings.check(status.level())?;

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
}

struct Environment {
    book_src: Url,
    markdown: pulldown_cmark::Options,
    vcs: VersionControl,
    config: Config,
}

struct VersionControl {
    root: Url,
    link: Permalink,
}

impl Environment {
    fn resolve(&self, content: &mut Pages<'_>) {
        self.validate();

        let fragments = content.take_fragments();

        for (base, link) in content.links_mut() {
            let file = if link.link.starts_with('/') {
                self.vcs.root.join(&link.link[1..])
            } else {
                base.join(&link.link)
            }
            .context("could not derive url")
            .tap_err(log_debug!());

            let Ok(file_url) = file else {
                link.status = LinkStatus::Ignored;
                continue;
            };

            let env = self;
            let page_url = base.as_ref();
            let fragments = &fragments;

            Resolver {
                link,
                page_url,
                file_url,
                env,
                fragments,
            }
            .resolve();
        }

        let mut fragments = fragments;
        fragments.restore(content);
        drop(fragments);
    }

    #[inline]
    fn validate(&self) {
        debug_assert!(
            self.book_src.as_str().ends_with('/'),
            "book_src should have a trailing slash, got {}",
            self.book_src
        );
        debug_assert!(
            self.vcs.root.as_str().ends_with('/'),
            "vcs_root should have a trailing slash, got {}",
            self.vcs.root
        );
    }

    fn new(book: &PreprocessorContext) -> Result<Result<Self>> {
        let config = config_from_book::<Config>(&book.config, "link-forever")
            .context("failed to read preprocessor config from book.toml")?;

        let vcs = match VersionControl::try_from_git(&config, &book.config) {
            Ok(Ok(vcs)) => vcs,
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };

        let markdown = mdbook_markdown_options().tap_mut(|m| {
            if smart_punctuation(&book.config) {
                m.insert(pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION);
            }
        });

        let book_src = book
            .root
            .canonicalize()
            .context("failed to locate book root")?
            .join(&book.config.book.src)
            .pipe(Url::from_directory_path)
            .map_err(|_| anyhow!("book `src` should be a valid absolute path"))?;

        Ok(Ok(Self {
            book_src,
            markdown,
            vcs,
            config,
        }))
    }
}

#[must_use]
struct Resolver<'a, 'r> {
    file_url: Url,
    page_url: &'a Url,
    link: &'a mut RelativeLink<'r>,
    env: &'a Environment,
    fragments: &'a Fragments,
}

impl Resolver<'_, '_> {
    fn resolve(self) {
        if let Some(book) = &self.env.config.book_url {
            if let Some(page) = book.0.make_relative(&self.file_url) {
                // hard-coded URLs to book pages
                self.resolve_page(page)
            } else {
                self.resolve_file()
            }
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
            fragments,
        } = self;

        let file_url = if let Some(path) = env.vcs.link.to_path(&file_url) {
            if let Ok(file_url) = env.vcs.root.join(&path) {
                link.link = format!("/{path}").into();
                file_url
            } else {
                file_url
            }
        } else {
            file_url
        };

        let Ok(path) = file_url.to_file_path() else {
            link.status = LinkStatus::Ignored;
            return;
        };

        let Ok(relative_to_repo) = env
            .vcs
            .root
            .make_relative(&file_url)
            .context("url is from a different origin")
            .tap_err(log_debug!())
        else {
            return;
        };

        if relative_to_repo.starts_with("../") {
            // `path` could also be a symlink to a file outside source control somehow
            // in which case it would NOT be marked as LinkStatus::External;
            link.status = LinkStatus::PathNotCheckedIn;
            return;
        }

        let exists = path
            .try_exists()
            .context("could not access path")
            .tap_err(log_debug!());

        if !matches!(exists, Ok(true)) {
            link.status = LinkStatus::NoSuchPath;
            return;
        }

        let Ok(relative_to_book) = env
            .book_src
            .make_relative(&file_url)
            .context("url is from a different origin")
            .tap_err(log_debug!())
        else {
            return;
        };

        let always_link = relative_to_book.starts_with("../")
            || env
                .config
                .always_link
                .iter()
                .any(|suffix| file_url.path().ends_with(suffix));

        if !always_link {
            if link.link.starts_with('/') {
                // mdbook doesn't support absolute paths like VS Code does
                link.link = page_url.make_relative(&file_url).unwrap().into();
                link.status = LinkStatus::Rewritten;
            } else {
                link.status = LinkStatus::Published;
            }
            Self {
                link,
                page_url,
                file_url,
                env,
                fragments,
            }
            .resolve_fragment();
            return;
        }

        match env.vcs.link.to_link(&relative_to_repo) {
            Ok(href) => {
                link.status = LinkStatus::Permalink;
                link.link = href.as_str().to_owned().into();
            }
            Err(err) => link.status = LinkStatus::Error(format!("{err}")),
        }
    }

    /// Check hard-coded URLs to book pages
    fn resolve_page(self, page: String) {
        let Self {
            file_url,
            page_url,
            link,
            env,
            fragments,
        } = self;

        let path = {
            let mut path = page;
            if let Some(idx) = path.find('#') {
                path.truncate(idx)
            };
            if let Some(idx) = path.find('?') {
                path.truncate(idx)
            };
            path.strip_suffix(".html")
                .map(ToOwned::to_owned)
                .unwrap_or(path)
        };

        if path.starts_with("../") {
            link.status = LinkStatus::Ignored;
            return;
        }

        let mut not_found = vec![];

        // one does not simply avoid trailing slash issues...
        // https://github.com/slorber/trailing-slash-guide
        let try_files = if path.is_empty() || path.ends_with('/') {
            &[
                // enforce that index.html pages should consistently
                // be addressed with a trailing slash
                format!("{path}index.md"),
                format!("{path}README.md"),
            ] as &[String]
        } else {
            &[
                format!("{path}.md"),
                // all major hosting providers implicitly redirect
                // /folder to /folder/, so these are okay
                format!("{path}/index.md"),
                format!("{path}/README.md"),
            ]
        };

        for file in try_files {
            let Ok(file) = self.env.book_src.join(file).tap_err(log_debug!()) else {
                continue;
            };

            let Ok(path) = file.to_file_path() else {
                continue;
            };

            let exists = path
                .try_exists()
                .context("could not access path")
                .tap_err(log_debug!());

            if matches!(exists, Ok(true)) {
                let file_url = {
                    let mut file = file;
                    file.set_query(file_url.query());
                    file.set_fragment(file_url.fragment());
                    file
                };

                link.link = page_url.make_relative(&file_url).unwrap().into();
                link.status = LinkStatus::Rewritten;

                Self {
                    link,
                    page_url,
                    file_url,
                    env,
                    fragments,
                }
                .resolve_fragment();

                return;
            }

            not_found.push(file);
        }

        link.link = not_found[0].to_string().into();
        link.status = LinkStatus::NoSuchPath;
    }

    fn resolve_fragment(self) {
        let Self {
            mut file_url,
            link,
            fragments,
            ..
        } = self;

        let Some(fragment) = file_url
            .fragment()
            .and_then(|f| percent_decode_str(f).decode_utf8().ok().or(Some(f.into())))
            .map(|f| f.into_owned())
        else {
            return;
        };

        file_url.set_fragment(None);

        let found = fragments.contains(&file_url, &fragment);

        file_url.set_fragment(Some(&fragment));

        if !found {
            link.status = LinkStatus::NoSuchFragment;
        }
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
