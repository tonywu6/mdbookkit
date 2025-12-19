use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use tap::{Pipe, TapFallible};
use url::Url;

use mdbookkit::{
    log_warning,
    markdown::{PatchStream, Spanned},
};

use crate::link::{ContentTypeHint, EmitLinkSpan, LinkSpan, LinkStatus, LinkText, RelativeLink};

pub struct Pages<'a> {
    pages: HashMap<Arc<Url>, Page<'a>>,
    markdown: Options,
}

struct Page<'a> {
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
            .keys()
            .filter_map(|url| root.make_relative(url))
            .collect()
    }

    pub fn insert(&mut self, url: Url, source: &'a str) -> Result<&mut Self> {
        let stream = Parser::new_ext(source, self.markdown).into_offset_iter();
        let page = Page::read(source, stream)?;
        self.pages.insert(url.into(), page);
        Ok(self)
    }

    pub fn links(&'_ self) -> impl Iterator<Item = (&'_ Arc<Url>, &'_ RelativeLink<'_>)> {
        self.pages.iter().flat_map(|(base, page)| {
            page.links
                .iter()
                .flat_map(move |links| links.links().map(move |link| (base, link)))
        })
    }

    pub fn links_mut(&mut self) -> impl Iterator<Item = (&Arc<Url>, &mut RelativeLink<'a>)> {
        self.pages.iter_mut().flat_map(|(base, page)| {
            page.links
                .iter_mut()
                .flat_map(move |links| links.links_mut().map(move |link| (base, link)))
        })
    }

    pub fn get_text(&self, url: &Url) -> Option<&str> {
        self.pages.get(url).map(|page| page.source)
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
                    let (usage, link, title) = match tag {
                        Tag::Link {
                            dest_url, title, ..
                        } => (ContentTypeHint::Tree, dest_url, title),
                        Tag::Image {
                            dest_url, title, ..
                        } => (ContentTypeHint::Raw, dest_url, title),
                        _ => unreachable!(),
                    };
                    let link = RelativeLink {
                        status: LinkStatus::Ignored,
                        span,
                        link,
                        hint: usage,
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
                        TagEnd::Link => ContentTypeHint::Tree,
                        TagEnd::Image => ContentTypeHint::Raw,
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

                event => {
                    if let Some(link) = opened.as_mut() {
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
            .into_string()
            .tap_err(log_warning!())?
            .pipe(Ok)
    }
}
