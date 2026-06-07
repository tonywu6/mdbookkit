use std::{collections::HashMap, fmt::Debug};

use anyhow::{Result, bail};
use mdbook_markdown::pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use tap::{Pipe, Tap};
use tracing::{debug, info, instrument, trace};
use url::Url;

use mdbookkit::{
    error::Show,
    markdown::{PatchStream, Spanned},
    plural,
    url::{RelativeUrl, UrlUtil},
};

use crate::{
    link::{ContentKind, EmitLinkSpan, Link, LinkSpan, LinkState, LinkText},
    vcs::VersionControl,
};

pub struct Pages<'a> {
    root: Url,
    pages: Vec<(Url, Page<'a>)>,
    markdown: Options,
}

pub struct Page<'a> {
    source: &'a str,
    links: Vec<LinkSpan<'a>>,
}

impl<'a> Pages<'a> {
    pub fn new(root: Url, markdown: Options) -> Self {
        Self {
            root: root.with_trailing_slash(),
            pages: Default::default(),
            markdown,
        }
    }

    pub fn insert(&mut self, url: Url, source: &'a str) -> Result<()> {
        let stream = Parser::new_ext(source, self.markdown).into_offset_iter();
        let page = Page::read(source, stream)?;
        self.pages.push((url, page));
        Ok(())
    }

    pub fn pages(&self) -> impl Iterator<Item = &(Url, Page<'a>)> {
        self.pages.iter()
    }

    pub fn links_mut(&mut self) -> impl Iterator<Item = (&Url, &mut Link<'a>)> {
        self.pages.iter_mut().flat_map(|(base, page)| {
            let base = &*base;
            (page.links.iter_mut())
                .flat_map(move |links| links.links_mut().map(move |link| (base, link)))
        })
    }

    pub fn emit(self) -> HashMap<Url, Result<String>> {
        self.pages
            .into_iter()
            .map(|(key, page)| (key, page.emit()))
            .collect()
    }

    pub fn log_stats(&self) {
        let mut ignored = 0;
        let mut unchanged = 0;
        let mut rewritten = 0;
        let mut permalink = 0;
        let mut error = 0;
        let mut total = 0;

        for (_, page) in self.pages() {
            for link in page.links() {
                total += 1;
                match link.state() {
                    Ok(LinkState::Unsupported) => ignored += 1,
                    Ok(LinkState::BookLinkChecked) => unchanged += 1,
                    Ok(LinkState::BookLinkUpdated) => rewritten += 1,
                    Ok(LinkState::Permalink) => permalink += 1,
                    Err(..) => error += 1,
                }
            }
        }

        info!(
            "processed {total}: {permalink} to repo; {rewritten} to book; {error}; {unchanged}",
            total = plural!(total, "link"),
            permalink = plural!(permalink, "link"),
            rewritten = plural!(rewritten, "link"),
            error = plural!(error, "has error", "have errors"),
            unchanged = plural!(unchanged + ignored, "unchanged", "unchanged"),
        );
    }

    pub fn root(&self) -> &Url {
        &self.root
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
        };

        let mut opened: Option<LinkSpan<'_>> = None;

        for (event, span) in stream {
            match event {
                Event::Start(tag @ (Tag::Link { .. } | Tag::Image { .. })) => {
                    let (kind, dest, title) = match tag {
                        Tag::Link {
                            dest_url, title, ..
                        } => (ContentKind::Web, dest_url, title),
                        Tag::Image {
                            dest_url, title, ..
                        } => (ContentKind::Raw, dest_url, title),
                        _ => unreachable!(),
                    };

                    let parent = opened.as_ref().map(|link| link.span());
                    trace!(?span, ?parent, ?kind, ">>>");
                    trace!(?dest, " │ ");
                    trace!(?title, " │ ");

                    let link = Link::builder()
                        .href(dest.clone())
                        .span(span)
                        .kind(kind)
                        .title(title)
                        .source(source)
                        .build()
                        .pipe(Box::new)
                        .pipe(LinkText::Link);

                    match opened.as_mut() {
                        Some(opened) => opened.0.push(link),
                        None => opened = Some(LinkSpan(vec![link])),
                    }
                }

                event @ Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                    let Some(mut items) = opened.take() else {
                        debug!(?span, "unexpected {end:?}");
                        bail!("markdown stream malformed at byte position {span:?}");
                    };

                    trace!(?span, "<<<");

                    items.0.push(LinkText::Text(event));

                    if &span == items.span() {
                        this.links.push(items);
                    } else {
                        opened = Some(items)
                    }
                }

                event => {
                    if let Some(link) = opened.as_mut() {
                        trace!(?span, " │ ");
                        link.0.push(LinkText::Text(event))
                    }
                }
            }
        }

        Ok(this)
    }

    fn emit(&self) -> Result<String> {
        self.links
            .iter()
            .filter_map(EmitLinkSpan::new)
            .pipe(|stream| PatchStream::new(self.source, stream))
            .into_string()?
            .pipe(Ok)
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn links(&self) -> impl Iterator<Item = &Link<'a>> {
        self.links.iter().flat_map(|span| span.links())
    }
}

pub struct BookPaths {
    root: RelativeUrl,
    source_paths: HashMap<String, Url>,
    public_paths: HashMap<String, Url>,
}

impl Pages<'_> {
    pub fn book_paths(&self, vcs: &VersionControl) -> BookPaths {
        let mut source_paths = HashMap::new();
        let mut public_paths = HashMap::new();

        let root = vcs.root().as_base();

        for (url, _) in self.pages.iter() {
            if (url.path().ends_with("/index.md") || url.path().ends_with("/README.md"))
                && let Ok(mut path) = url.join(".")
            {
                path.ensure_trailing_slash();
                if let Some(href) = root.make_relative(&path) {
                    let href = href.encoded_path().to_owned();
                    public_paths.insert(href, url.clone());
                }
                path.ensure_no_trailing_slash();
                if let Some(href) = root.make_relative(&path) {
                    let href = href.encoded_path().to_owned();
                    public_paths.insert(href, url.clone());
                }
            }

            if let Some(href) = root.make_relative(url) {
                let href = href.encoded_path().to_owned();
                if let Some(href) = href.strip_suffix(".md") {
                    public_paths.insert(format!("{href}.html"), url.clone());
                    public_paths.insert(href.to_owned(), url.clone());
                }

                source_paths.insert(href, url.clone());
            }
        }

        let root = (vcs.root().as_base())
            .make_relative(&self.root)
            .expect("`page_dir` should be under source control");

        BookPaths {
            root,
            source_paths,
            public_paths,
        }
    }
}

impl BookPaths {
    #[instrument(level = "trace", "book_try_file", skip_all, fields(path = ?url.show_path()))]
    pub fn try_file(&self, url: &RelativeUrl) -> Option<TryBookPath> {
        let root = self.root.encoded_path();
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

    pub fn source_paths_for(url: &Url) -> Vec<Url> {
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
pub enum TryBookPath {
    NoSuchPage,
    SourcePath { resolved: Url },
    PublicPath { resolved: Url },
}

impl Debug for BookPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookPaths")
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
