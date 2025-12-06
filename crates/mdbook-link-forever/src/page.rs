use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    ops::Range,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use mdbook::utils::unique_id_from_content;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html::push_html};
use tap::{Pipe, Tap, TapFallible};
use url::Url;

use mdbookkit::{
    log_warning,
    markdown::{PatchStream, Spanned},
};

use crate::link::{EmitLinkSpan, LinkSpan, LinkStatus, LinkText, LinkUsage, RelativeLink};

pub struct Pages<'a> {
    pages: HashMap<Arc<Url>, Page<'a>>,
    markdown: Options,
}

struct Page<'a> {
    source: &'a str,
    links: Vec<LinkSpan<'a>>,
    fragments: HashSet<String>,
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

    pub fn take_fragments(&mut self) -> Fragments {
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
        let fragment = String::new().tap_mut(|s| push_html(s, heading.cloned()));
        let fragment = unique_id_from_content(&fragment, counter);
        self.fragments.insert(fragment);
    }

    fn insert_id(&mut self, id: &str, counter: &mut HashMap<String, usize>) {
        counter.insert(id.into(), 1);
        self.fragments.insert(id.into());
    }
}

#[must_use]
pub struct Fragments(HashMap<Arc<Url>, HashSet<String>>);

impl Fragments {
    pub fn contains(&self, page: &Url, fragment: &str) -> bool {
        self.0
            .get(page)
            .map(|f| f.contains(fragment))
            .unwrap_or(false)
    }

    pub fn restore(&mut self, pages: &mut Pages<'_>) {
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
