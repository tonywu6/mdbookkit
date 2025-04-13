use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    ops::{ControlFlow, Range},
    str::FromStr,
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use mdbook::utils::unique_id_from_content;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible};
use url::{form_urlencoded::Serializer as SearchParams, Url};

use crate::{
    env::ErrorHandling,
    log_debug, log_warning,
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
    /// Try to convert this path to a permalink
    fn to_link(&self, path: &str) -> Result<Url>;
    /// Try to extract a path (relative to repo root) from this link
    fn to_path(&self, link: &Url) -> Option<String>;
}

#[derive(Debug, Default, Clone, thiserror::Error)]
pub enum LinkStatus {
    #[default]
    #[error("link is ignored as it is not supported")]
    Ignored,

    // "published" as in published with the book
    #[error("link to book page or file")]
    Published,

    #[error("link to book page or file rewritten as path")]
    Rewritten,
    #[error("link converted to permalink")]
    Permalink,

    #[error("path to a file outside source control")]
    PathNotCheckedIn,
    #[error("file does not exist at path")]
    NoSuchPath,
    #[error("fragment does not exist in page")]
    NoSuchFragment,

    #[error("error generating a link: {0}")]
    Error(String),
}

impl Environment {
    pub fn resolve(&self, content: &mut Pages<'_>) {
        self.validate_self();

        let fragments = content.take_fragments();

        let iter = content.pages.iter_mut().flat_map(|(base, page)| {
            page.links
                .iter_mut()
                .flat_map(move |links| links.links_mut().map(move |link| (base, link)))
        });

        for (base, link) in iter {
            let file = if link.link.starts_with('/') {
                self.vcs_root.join(&link.link[1..])
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

        let file_url = if let Some(path) = env.fmt_link.to_path(&file_url) {
            if let Ok(file_url) = env.vcs_root.join(&path) {
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

        let always_link = rel.starts_with("../")
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

        let Ok(rel) = env
            .vcs_root
            .make_relative(&file_url)
            .context("url is from a different origin")
            .tap_err(log_debug!())
        else {
            return;
        };

        if rel.starts_with("../") {
            // `path` could also be a symlink to a file outside source control somehow
            // in which case it would NOT be marked as LinkStatus::External;
            link.status = LinkStatus::PathNotCheckedIn;
            return;
        }

        match env.fmt_link.to_link(&rel) {
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
    links: Vec<LinkSpan<'a>>,
    fragments: HashSet<String>,
}

struct LinkSpan<'a>(Vec<LinkText<'a>>);

enum LinkText<'a> {
    Text(Event<'a>),
    Link(RelativeLink<'a>),
}

struct RelativeLink<'a> {
    status: LinkStatus,
    span: Range<usize>,
    link: CowStr<'a>,
    usage: LinkUsage,
    title: CowStr<'a>,
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
            links: Default::default(),
            fragments: Default::default(),
        };

        struct Heading<'a> {
            span: Range<usize>,
            text: Vec<Event<'a>>,
        }

        let mut heading: Option<Heading<'_>> = None;
        let mut counter: HashMap<String, usize> = Default::default();

        let mut opened: Option<LinkSpan<'_>> = None;

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
                    let (usage, link, title) = match tag {
                        Tag::Link {
                            dest_url, title, ..
                        } => (LinkUsage::Link, dest_url, title),
                        Tag::Image {
                            dest_url, title, ..
                        } => (LinkUsage::Image, dest_url, title),
                        _ => unreachable!(),
                    };
                    let link = RelativeLink {
                        status: LinkStatus::PathNotCheckedIn,
                        span,
                        link,
                        usage,
                        title,
                    }
                    .pipe(LinkText::Link);
                    match opened.as_mut() {
                        Some(opened) => opened.0.push(link),
                        None => opened = Some(LinkSpan(vec![link])),
                    }
                }

                event @ Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                    let usage = match end {
                        TagEnd::Link => LinkUsage::Link,
                        TagEnd::Image => LinkUsage::Image,
                        _ => unreachable!(),
                    };
                    let Some(mut items) = opened.take() else {
                        bail!("unexpected {usage:?} at {span:?}")
                    };
                    items.0.push(LinkText::Text(event));
                    if &span == items.span() {
                        this.links.push(items);
                    } else {
                        opened = Some(items)
                    }
                }

                event => match (heading.as_mut(), opened.as_mut()) {
                    (Some(heading), Some(link)) => {
                        heading.text.push(event.clone());
                        link.0.push(LinkText::Text(event));
                    }
                    (Some(heading), None) => {
                        heading.text.push(event);
                    }
                    (None, Some(link)) => {
                        link.0.push(LinkText::Text(event));
                    }
                    (None, None) => {}
                },
            }
        }

        Ok(this)
    }

    fn emit(&self) -> Result<String> {
        self.links
            .iter()
            .filter_map(EmitLinkSpan::new)
            .pipe(|stream| PatchStream::new(self.source, stream))
            .into_string()
            .tap_err(log_warning!())?
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

impl<'a> LinkSpan<'a> {
    fn links_mut(&mut self) -> impl Iterator<Item = &'_ mut RelativeLink<'a>> {
        self.0.iter_mut().filter_map(|item| match item {
            LinkText::Link(link) => Some(link),
            LinkText::Text(..) => None,
        })
    }

    fn span(&self) -> &Range<usize> {
        match &self.0[0] {
            LinkText::Link(link) => &link.span,
            LinkText::Text(..) => unreachable!("first item in LinkSpan must be a Link"),
        }
    }
}

struct EmitLinkSpan<'a> {
    iter: std::slice::Iter<'a, LinkText<'a>>,
    opened: Vec<LinkUsage>,
}

impl<'a> Iterator for EmitLinkSpan<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            match next {
                LinkText::Text(text) => {
                    match (text, self.opened.last()) {
                        (Event::End(TagEnd::Link), Some(LinkUsage::Link)) => {
                            self.opened.pop();
                            return Some(text.clone());
                        }
                        (Event::End(TagEnd::Image), Some(LinkUsage::Image)) => {
                            self.opened.pop();
                            return Some(text.clone());
                        }
                        (Event::End(TagEnd::Link | TagEnd::Image), None) => {
                            // skip this end tag because the link was skipped
                            continue;
                        }
                        _ => {
                            return Some(text.clone());
                        }
                    };
                }
                LinkText::Link(link) => {
                    match (link.will_emit(), self.opened.is_empty()) {
                        (Some(usage), _) => {
                            self.opened.push(usage);
                            return Some(Event::Start(link.emit()));
                        }
                        (None, false) => {
                            return Some(Event::Start(link.emit()));
                        }
                        (None, true) => {
                            continue;
                        }
                    };
                }
            };
        }
        None
    }
}

impl<'a> EmitLinkSpan<'a> {
    fn new(links: &'a LinkSpan<'a>) -> Option<(Self, Range<usize>)> {
        let span = links.0.iter().find_map(|link| match &link {
            LinkText::Link(link) => {
                if link.will_emit().is_some() {
                    Some(link.span.clone())
                } else {
                    None
                }
            }
            _ => None,
        })?;
        let iter = EmitLinkSpan {
            iter: links.0.iter(),
            opened: vec![],
        };
        Some((iter, span))
    }
}

impl RelativeLink<'_> {
    fn emit(&self) -> Tag<'_> {
        match self.usage {
            LinkUsage::Link => Tag::Link {
                link_type: LinkType::Inline,
                dest_url: self.link.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
            LinkUsage::Image => Tag::Image {
                link_type: LinkType::Inline,
                dest_url: self.link.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
        }
    }

    fn will_emit(&self) -> Option<LinkUsage> {
        if matches!(self.status, LinkStatus::Permalink | LinkStatus::Rewritten) {
            Some(self.usage)
        } else {
            None
        }
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

/// Configuration for the preprocessor.
///
/// This is deserialized from book.toml.
///
/// Doc comments for attributes populate the `configuration.md` page in the docs.
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

pub struct GitHubPermalink(CustomPermalink);

impl PermalinkFormat for GitHubPermalink {
    fn to_link(&self, path: &str) -> Result<Url> {
        self.0.to_link(path)
    }

    fn to_path(&self, link: &Url) -> Option<String> {
        self.0.to_path(link)
    }
}

impl GitHubPermalink {
    /// c.f. <https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content--parameters>
    pub fn new(owner: &str, repo: &str, reference: &str) -> Self {
        let pattern = format!("https://github.com/{owner}/{repo}/{{tree}}/{{ref}}/{{path}}")
            .parse()
            .expect("should be a valid url");
        let reference = reference.into();
        Self(CustomPermalink { pattern, reference })
    }
}

pub struct CustomPermalink {
    pub pattern: Url,
    pub reference: String,
}

/// `{` and `}` are always percent-encoded in path [^1].
///
/// Encoding characters are always in uppercase [^2].
///
/// [^1]: https://url.spec.whatwg.org/#path-percent-encode-set
/// [^2]: https://url.spec.whatwg.org/#percent-encode
macro_rules! encoded_param {
    ($param:literal) => {
        concat!("%7B", $param, "%7D")
    };
}

impl PermalinkFormat for CustomPermalink {
    fn to_link(&self, path: &str) -> Result<Url> {
        let path = self
            .pattern
            .path()
            .split('/')
            .map(|segment| match segment {
                encoded_param!("ref") => &self.reference,
                encoded_param!("tree") => "tree",
                encoded_param!("path") => path,
                _ => segment,
            })
            .collect::<Vec<_>>()
            .join("/");

        let query = self
            .pattern
            .query_pairs()
            .fold(SearchParams::new(String::new()), |mut search, (k, v)| {
                match v.as_ref() {
                    "{ref}" => search.append_pair(&k, &self.reference),
                    "{tree}" => search.append_pair(&k, "tree"),
                    "{path}" => search.append_pair(&k, &path),
                    _ => search.append_pair(&k, &v),
                };
                search
            })
            .finish()
            .pipe(|query| if query.is_empty() { None } else { Some(query) });

        let fragment = self.pattern.fragment();

        self.pattern
            .clone()
            .tap_mut(|u| u.set_path(&path))
            .tap_mut(|u| u.set_query(query.as_deref()))
            .tap_mut(|u| u.set_fragment(fragment))
            .pipe(Ok)
    }

    // this is kind of ugly
    fn to_path(&self, link: &Url) -> Option<String> {
        if self.pattern.origin() != link.origin() {
            return None;
        }

        fn match_param(lhs: &str, rhs: Option<&str>) -> ControlFlow<()> {
            match lhs {
                encoded_param!("tree") => match rhs {
                    Some("tree" | "blob" | "raw") => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
                encoded_param!("ref") => match rhs {
                    Some("HEAD") => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
                lhs => match rhs {
                    Some(rhs) if lhs == rhs => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
            }
        }

        let mut path = false;

        let mut lhs = self.pattern.path().split('/');
        let mut rhs = link.path().split('/');

        #[allow(clippy::while_let_on_iterator, reason = "symmetry")]
        while let Some(lhs) = lhs.next() {
            match lhs {
                encoded_param!("path") => {
                    path = true;
                    break;
                }
                lhs => match match_param(lhs, rhs.next()) {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(()) => return None,
                },
            }
        }

        while let Some(lhs) = lhs.next_back() {
            match match_param(lhs, rhs.next_back()) {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return None,
            }
        }

        let mut path = if path {
            Some(rhs.collect::<Vec<_>>().join("/"))
        } else {
            None
        };

        let link_query = link.query_pairs().collect::<HashMap<_, _>>();

        for (k, v) in self.pattern.query_pairs() {
            match v.as_ref() {
                "{path}" => match link_query.get(&k) {
                    Some(v) => {
                        path = if let Some(v) = v.strip_prefix('/') {
                            Some(v.into())
                        } else {
                            Some(v.as_ref().into())
                        }
                    }
                    None => return None,
                },
                "{tree}" => match link_query.get(&k).map(|v| &**v) {
                    Some("tree" | "blob" | "raw") => {}
                    _ => return None,
                },
                "{ref}" => match link_query.get(&k).map(|v| &**v) {
                    Some("HEAD") => {}
                    _ => return None,
                },
                _ => {}
            }
        }

        if let Some(path) = path {
            match link.fragment() {
                Some(fragment) => Some(format!("{path}#{fragment}")),
                None => Some(path),
            }
        } else {
            None
        }
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

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{CustomPermalink, PermalinkFormat};

    #[test]
    fn test_path_to_link() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let link = scheme.to_link(".editorconfig")?;

        assert_eq!(
            link.as_str(),
            "https://github.com/lorem/ipsum/tree/main/.editorconfig"
        );

        Ok(())
    }

    #[test]
    fn test_path_to_link_with_suffix() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "master".into(),
        };

        let link = scheme.to_link(".editorconfig")?;

        assert_eq!(link.as_str(), "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=master");

        Ok(())
    }

    #[test]
    fn test_link_to_path() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD/path/to/file".parse()?);

        assert_eq!(path.as_deref(), Some("path/to/file"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_with_fragments() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(
            &"https://github.com/lorem/ipsum/blob/HEAD/path/to/file.md#section-1".parse()?,
        );

        assert_eq!(path.as_deref(), Some("path/to/file.md#section-1"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_repo_root() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD".parse()?);

        assert_eq!(path.as_deref(), Some(""));

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD/".parse()?);

        assert_eq!(path.as_deref(), Some(""));

        Ok(())
    }

    #[test]
    fn test_link_to_path_with_suffix() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let path =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=HEAD".parse()?);

        assert_eq!(path.as_deref(), Some(".editorconfig"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_non_head() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let matched =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=b676ac4".parse()?);

        assert!(matched.is_none());

        Ok(())
    }
}
