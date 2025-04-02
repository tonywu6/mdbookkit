use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    ops::Range,
    str::FromStr,
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use mdbook::utils::unique_id_from_content;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible};
use url::Url;

use crate::{
    env::ErrorHandling,
    log_debug,
    markdown::{PatchStream, Spanned},
};

#[cfg(feature = "common-logger")]
mod diagnostic;
#[cfg(feature = "link-forever")]
mod git;

pub struct Environment {
    pub book_src: Url,
    pub vcs_root: Url,
    pub fmt_link: Box<dyn PermalinkFormat>,
    pub markdown: pulldown_cmark::Options,
    pub config: Config,
}

pub trait PermalinkFormat {
    fn link_to(&self, relpath: &str) -> Result<Url>;
}

impl Environment {
    pub fn resolve(&self, content: &mut Pages<'_>) {
        self.validate_self();

        let fragments = content.take_fragments();

        for (base, link) in content
            .pages
            .iter_mut()
            .flat_map(|(base, page)| page.rel_links.iter_mut().map(move |link| (base, link)))
        {
            let file = if link.link.starts_with('/') {
                self.vcs_root.join(&link.link[1..])
            } else {
                base.join(&link.link)
            }
            .context("couldn't derive url")
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

        fragments.tap_mut(|f| f.restore(content));
    }

    #[inline]
    fn validate_self(&self) {
        debug_assert!(
            self.book_src.as_str().ends_with('/'),
            "book_src should have a trailing slash, got {}",
            self.book_src
        );
        debug_assert!(
            self.vcs_root.as_str().ends_with('/'),
            "vcs_root should have a trailing slash, got {}",
            self.vcs_root
        );
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
        if self.file_url.scheme() == "file" {
            self.resolve_file()
        } else if let Some(book) = &self.env.config.book_url {
            if let Some(page) = book.0.make_relative(&self.file_url) {
                self.resolve_page(page);
            } else {
                self.link.status = LinkStatus::Ignored;
            }
        } else {
            self.link.status = LinkStatus::Ignored
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

        let Ok(path) = file_url.to_file_path() else {
            link.status = LinkStatus::Ignored;
            return;
        };

        let exists = path
            .try_exists()
            .context("could not access path")
            .tap_err(log_debug!());

        if !matches!(exists, Ok(true)) {
            link.status = LinkStatus::NoSuchPath;
            return;
        }

        let Ok(rel) = env
            .book_src
            .make_relative(&file_url)
            .context("url is from a different origin")
            .tap_err(log_debug!())
        else {
            return;
        };

        let always_link = env
            .config
            .always_link
            .iter()
            .any(|suffix| file_url.path().ends_with(suffix));

        if !always_link && !rel.starts_with("../") {
            if link.link.starts_with('/') {
                // mdbook doesn't support absolute paths like VS Code does
                link.link = page_url.make_relative(&file_url).unwrap().into();
                link.status = LinkStatus::Rewritten
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

        let Ok(rel) = env
            .vcs_root
            .make_relative(&file_url)
            .context("url is from a different origin")
            .tap_err(log_debug!())
        else {
            return;
        };

        if rel.starts_with("../") {
            link.status = LinkStatus::External;
            return;
        }

        match env.fmt_link.link_to(&rel) {
            Ok(href) => {
                link.status = LinkStatus::Permalink;
                link.link = href.as_str().to_owned().into();
            }
            Err(err) => link.status = LinkStatus::Error(format!("{err}")),
        }
    }

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

        for file in [
            format!("{path}.md"),
            format!("{path}/index.md"),
            format!("{path}/README.md"),
        ] {
            let Ok(file) = self.env.book_src.join(&file).tap_err(log_debug!()) else {
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

                link.link = file_url.to_string().into();
                link.status = LinkStatus::Published;

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

        let found = fragments
            .0
            .get(&file_url)
            .map(|f| f.contains(&fragment))
            .unwrap_or(false);

        file_url.set_fragment(Some(&fragment));

        if !found {
            link.status = LinkStatus::NoSuchFragment;
        }
    }
}

pub struct Pages<'a> {
    pages: HashMap<Arc<Url>, Page<'a>>,
    markdown: Options,
}

struct Page<'a> {
    source: &'a str,
    rel_links: Vec<RelativeLink<'a>>,
    fragments: HashSet<String>,
}

struct RelativeLink<'a> {
    span: Range<usize>,
    link: CowStr<'a>,
    usage: LinkUsage,
    title: CowStr<'a>,
    inner: Vec<Event<'a>>,
    status: LinkStatus,
}

#[derive(Debug, Default, Clone)]
pub enum LinkStatus {
    /// Not a file: URL
    #[default]
    Ignored,
    /// Link to a file under src/
    Published,
    /// Link to a file under src/ but was rewritten
    Rewritten,
    /// Link to a file under source control
    Permalink,
    /// Link to a file outside source control
    External,
    /// Link to a file that cannot be accessed
    NoSuchPath,
    /// Link to a fragment that does not exist in a page
    NoSuchFragment,
    /// Link to a file under source control but link generation failed
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
enum LinkUsage {
    Link,
    Image,
}

impl<'a> Pages<'a> {
    pub fn new(markdown: Options) -> Self {
        Self {
            pages: Default::default(),
            markdown,
        }
    }

    pub fn insert(&mut self, url: Url, source: &'a str) -> Result<&mut Self> {
        let stream = Parser::new_ext(source, self.markdown).into_offset_iter();
        let page = Page::read(source, stream)?;
        self.pages.insert(url.into(), page);
        Ok(self)
    }

    pub fn emit<Q>(&self, key: &Q) -> Result<String>
    where
        Arc<Url>: Borrow<Q>,
        Q: Eq + Hash + Debug + ?Sized,
    {
        let page = self.pages.get(key);
        let page = page.with_context(|| format!("no such document {key:?}"))?;
        page.emit()
    }

    #[must_use]
    fn take_fragments(&mut self) -> Fragments {
        self.pages
            .iter_mut()
            .map(|(url, page)| (url.clone(), std::mem::take(&mut page.fragments)))
            .collect::<HashMap<_, _>>()
            .pipe(Fragments)
    }
}

impl<'a> Page<'a> {
    fn read<S>(source: &'a str, stream: S) -> Result<Self>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        let mut this = Self {
            source,
            rel_links: Default::default(),
            fragments: Default::default(),
        };

        struct Heading<'a> {
            span: Range<usize>,
            text: Vec<Event<'a>>,
        }

        let mut heading: Option<Heading<'_>> = None;
        let mut counter: HashMap<String, usize> = Default::default();

        let mut link: Option<RelativeLink<'_>> = None;

        for (event, span) in stream {
            match event {
                Event::Start(Tag::Heading { id, .. }) => {
                    if let Some(id) = id {
                        this.insert_id(&id, &mut counter);
                    } else if heading.is_some() {
                        bail!("unexpected `Tag::Heading` in `Tag::Heading` at {span:?}");
                    } else {
                        heading = Some(Heading { span, text: vec![] })
                    }
                }

                Event::End(TagEnd::Heading(..)) => {
                    let Some(Heading { span: start, text }) = heading.take() else {
                        bail!("unexpected `TagEnd::Heading` at {span:?}")
                    };
                    if start != span {
                        bail!("mismatching span, expected {start:?}, got {span:?}")
                    }
                    this.slugify(text.iter(), &mut counter);
                }

                Event::Start(tag @ (Tag::Link { .. } | Tag::Image { .. })) => {
                    let (usage, dest_url, title) = match tag {
                        Tag::Link {
                            dest_url, title, ..
                        } => (LinkUsage::Link, dest_url, title),
                        Tag::Image {
                            dest_url, title, ..
                        } => (LinkUsage::Image, dest_url, title),
                        _ => unreachable!(),
                    };
                    if link.is_some() {
                        bail!("unexpected {usage:?} in {usage:?} at {span:?}")
                    }
                    link = Some(RelativeLink {
                        span,
                        link: dest_url,
                        usage,
                        title,
                        inner: vec![],
                        status: LinkStatus::External,
                    });
                }

                Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                    let usage = match end {
                        TagEnd::Link => LinkUsage::Link,
                        TagEnd::Image => LinkUsage::Image,
                        _ => unreachable!(),
                    };
                    let Some(link) = link.take() else {
                        bail!("unexpected {usage:?} at {span:?}")
                    };
                    if link.span != span {
                        bail!("mismatching span, expected {:?}, got {span:?}", link.span);
                    }
                    if link.usage != usage {
                        bail!("unexpected {usage:?}, expected {:?}", link.usage)
                    }
                    this.rel_links.push(link);
                }

                event => match (heading.as_mut(), link.as_mut()) {
                    (Some(heading), Some(link)) => {
                        heading.text.push(event.clone());
                        link.inner.push(event);
                    }
                    (Some(heading), None) => {
                        heading.text.push(event);
                    }
                    (None, Some(link)) => {
                        link.inner.push(event);
                    }
                    (None, None) => {}
                },
            }
        }

        Ok(this)
    }

    fn emit(&self) -> Result<String> {
        self.rel_links
            .iter()
            .filter_map(|link| {
                if !matches!(link.status, LinkStatus::Permalink | LinkStatus::Rewritten) {
                    return None;
                }
                let start = match link.usage {
                    LinkUsage::Link => Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url: link.link.clone(),
                        title: link.title.clone(),
                        id: CowStr::Borrowed(""),
                    },
                    LinkUsage::Image => Tag::Image {
                        link_type: LinkType::Inline,
                        dest_url: link.link.clone(),
                        title: link.title.clone(),
                        id: CowStr::Borrowed(""),
                    },
                };
                let end = match link.usage {
                    LinkUsage::Link => TagEnd::Link,
                    LinkUsage::Image => TagEnd::Image,
                };
                std::iter::once(Event::Start(start))
                    .chain(link.inner.iter().cloned())
                    .chain(std::iter::once(Event::End(end)))
                    .pipe(|events| Some((events, link.span.clone())))
            })
            .pipe(|stream| PatchStream::new(self.source, stream))
            .into_string()?
            .pipe(Ok)
    }

    fn slugify<'r, S>(&mut self, heading: S, counter: &mut HashMap<String, usize>)
    where
        S: Iterator<Item = &'r Event<'r>>,
    {
        fn unmark<'a>(event: &'a Event<'_>) -> &'a str {
            match event {
                Event::Text(text) => text,
                Event::Code(text) => text,
                Event::InlineMath(text) => text,
                Event::DisplayMath(text) => text,
                Event::Html(html) => html,
                Event::InlineHtml(html) => html,
                Event::FootnoteReference(href) => href,
                _ => "",
            }
        }
        let fragment = heading.map(unmark).collect::<String>();
        let fragment = unique_id_from_content(&fragment, counter);
        self.fragments.insert(fragment);
    }

    fn insert_id(&mut self, id: &str, counter: &mut HashMap<String, usize>) {
        counter.insert(id.into(), 1);
        self.fragments.insert(id.into());
    }
}

impl Debug for LinkUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link => f.write_str("link"),
            Self::Image => f.write_str("image"),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "common-cli", derive(clap::Parser))]
pub struct Config {
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
    #[cfg_attr(
        feature = "common-cli",
        arg(long, value_name("FORMAT"), verbatim_doc_comment)
    )]
    pub repo_url_template: Option<String>,

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
    #[cfg_attr(
        feature = "common-cli",
        arg(long, value_name("URL"), verbatim_doc_comment)
    )]
    pub book_url: Option<UrlPrefix>,

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
    #[cfg_attr(
        feature = "common-cli",
        arg(
            long,
            value_delimiter(','),
            value_name("EXTENSIONS"),
            verbatim_doc_comment
        )
    )]
    pub always_link: Vec<String>,

    /// Exit with a non-zero status code when there are warnings.
    ///
    /// Warnings are always printed to the console regardless of this option.
    #[serde(default)]
    #[cfg_attr(feature = "common-cli", arg(long, value_enum, value_name("MODE"), default_value_t = Default::default()))]
    pub fail_on_warnings: ErrorHandling,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub after: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub before: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub renderers: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub command: Option<String>,
}

pub struct GitHubPermalink {
    owner: String,
    repo: String,
    reference: String,
}

impl PermalinkFormat for GitHubPermalink {
    fn link_to(&self, relpath: &str) -> Result<Url> {
        let owner = &self.owner;
        let repo = &self.repo;
        let reference = &self.reference;
        Ok(format!("https://github.com/{owner}/{repo}/tree/{reference}/{relpath}").parse()?)
    }
}

impl GitHubPermalink {
    pub fn new(owner: &str, repo: &str, reference: &str) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
            reference: reference.into(),
        }
    }
}

pub struct CustomPermalink {
    pub pattern: Url,
    pub reference: String,
}

impl PermalinkFormat for CustomPermalink {
    fn link_to(&self, relpath: &str) -> Result<Url> {
        Ok(self
            .pattern
            .as_str()
            .replace("{ref}", &self.reference)
            .replace("{path}", relpath)
            .parse()?)
    }
}

#[derive(Debug, Clone)]
pub struct UrlPrefix(Url);

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

struct Fragments(HashMap<Arc<Url>, HashSet<String>>);

impl Fragments {
    fn restore(&mut self, pages: &mut Pages<'_>) {
        let fragments = std::mem::take(&mut self.0);
        for (url, items) in fragments {
            pages.pages.get_mut(&url).unwrap().fragments = items;
        }
    }
}

/// Drop bomb
impl Drop for Fragments {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            unreachable!("page fragments were not restored")
        }
    }
}
