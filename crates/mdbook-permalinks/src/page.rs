use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Result, bail};
use mdbook_markdown::pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use tap::Pipe;
use tracing::{debug, info, instrument, trace};
use url::Url;

use mdbookkit::{
    markdown::{PatchStream, Spanned, locate_text},
    plural,
};

use crate::link::{
    ContentHint, EmitLinkSpan, LinkSpan, LinkStatus, LinkText, RelativeLink, SourceSpan,
};

pub struct Pages<'a> {
    pages: Vec<(Arc<Url>, Page<'a>)>,
    markdown: Options,
}

pub struct Page<'a> {
    source: &'a str,
    links: Vec<LinkSpan<'a>>,
}

impl<'a> Pages<'a> {
    pub fn new(markdown: Options) -> Self {
        Self {
            pages: Default::default(),
            markdown,
        }
    }

    pub fn paths(&self, root: &Url) -> HashSet<String> {
        self.pages
            .iter()
            .filter_map(|(url, _)| root.make_relative(url))
            .collect()
    }

    #[instrument(level = "debug", "page_read", skip_all)]
    pub fn insert(&mut self, url: Url, source: &'a str) -> Result<&mut Self> {
        debug!(path = ?url.path(), "reading file");
        let stream = Parser::new_ext(source, self.markdown).into_offset_iter();
        let page = Page::read(source, stream)?;
        self.pages.push((url.into(), page));
        Ok(self)
    }

    pub fn pages(&self) -> impl Iterator<Item = &(Arc<Url>, Page<'a>)> {
        self.pages.iter()
    }

    pub fn links(&self) -> impl Iterator<Item = (&Arc<Url>, &RelativeLink<'a>)> {
        self.pages.iter().flat_map(|(base, page)| {
            (page.links.iter()).flat_map(move |links| links.links().map(move |link| (base, link)))
        })
    }

    pub fn links_mut(&mut self) -> impl Iterator<Item = (&Arc<Url>, &mut RelativeLink<'a>)> {
        self.pages.iter_mut().flat_map(|(base, page)| {
            let base = &*base;
            (page.links.iter_mut())
                .flat_map(move |links| links.links_mut().map(move |link| (base, link)))
        })
    }

    pub fn emit(self) -> HashMap<Arc<Url>, Result<String>> {
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
        let mut unreachable = 0;
        let mut error = 0;
        let mut total = 0;

        for (_, link) in self.links() {
            total += 1;
            match link.status {
                LinkStatus::Ignored => ignored += 1,
                LinkStatus::Unchanged => unchanged += 1,
                LinkStatus::Rewritten => rewritten += 1,
                LinkStatus::Permalink => permalink += 1,
                LinkStatus::Unreachable(_) => unreachable += 1,
                LinkStatus::Error(_) => error += 1,
            }
        }

        info!(
            "processed {total}: {permalink} to repo; {rewritten} to book; {unreachable}; {unchanged}",
            total = plural!(total, "link"),
            permalink = plural!(permalink, "link"),
            rewritten = plural!(rewritten, "link"),
            unreachable = plural!(unreachable, "inaccessible path"),
            unchanged = plural!(unchanged + ignored + error, "unchanged", "unchanged"),
        );
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
                    let (hint, dest, title) = match tag {
                        Tag::Link {
                            dest_url, title, ..
                        } => (ContentHint::Tree, dest_url, title),
                        Tag::Image {
                            dest_url, title, ..
                        } => (ContentHint::Raw, dest_url, title),
                        _ => unreachable!(),
                    };

                    let parent = opened.as_ref().map(|link| link.span());
                    trace!(?span, ?parent, ?hint, ">>>");
                    trace!(?dest, " │ ");
                    trace!(?title, " │ ");

                    let link = RelativeLink {
                        status: LinkStatus::Ignored,
                        href: dest.clone(),
                        span: SourceSpan {
                            full: span,
                            link: locate_text(source, &dest),
                        },
                        hint,
                        title,
                    }
                    .pipe(LinkText::Link);

                    match opened.as_mut() {
                        Some(opened) => opened.0.push(link),
                        None => opened = Some(LinkSpan(vec![link])),
                    }
                }

                event @ Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                    let Some(mut items) = opened.take() else {
                        debug!(?span, "unexpected {end:?}");
                        bail!("Markdown stream malformed at byte position {span:?}");
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

    pub fn links(&self) -> impl Iterator<Item = &RelativeLink<'a>> {
        self.links.iter().flat_map(|span| span.links())
    }
}
