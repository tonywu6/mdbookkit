use std::{borrow::Cow, fmt::Debug, ops::Range};

use anyhow::{Result, bail};
use lol_html::{HtmlRewriter, Settings, element, html_content::Element};
use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use tracing::{debug, trace};
use url::Url;

use mdbookkit::{
    error::Show,
    markdown::{Spanned, locate_text},
    url::RelativeUrl,
};

use crate::Patch;

#[derive(Debug)]
pub struct Link<'a> {
    state: Result<LinkState, LinkError>,
    href: CowStr<'a>,
    interest: ContentInterest,
    span: LinkSpan,
    title: CowStr<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum LinkState {
    Unsupported,
    BookLinkChecked,
    BookLinkUpdated,
    Permalink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentInterest {
    Nav,
    Raw,
}

#[derive(Debug)]
pub enum LinkSpan {
    Exact(Range<usize>),
    Fuzzy(Range<usize>),
}

#[derive(Clone)]
pub struct LinkError {
    pub error: PathError,
    pub cause: Url,
    pub help: Option<LinkHelp>,
}

#[derive(Debug, Clone)]
pub enum PathError {
    NotInRepo,
    InvalidEncoding,
    Inaccessible(std::io::ErrorKind),
    NotFound,
    NotADirectory,
    GitIgnored,
    NoSuchPage(BookPathError),
    AmbiguousLinkToRoot,
}

#[derive(Debug, Clone)]
pub enum BookPathError {
    DirectoryHasNoIndexFile,
    MarkdownFileNotIncluded,
    UnexpectedFileExtension,
    NoResourceAtLocation(Vec<LinkError>),
}

#[derive(Debug, Clone)]
pub enum LinkHelp {
    FoundOther {
        from_page: RelativeUrl,
        from_repo: RelativeUrl,
    },
    GenericEdit {
        help: &'static str,
        edited: String,
    },
    LinkToRoot {
        to_repo: String,
        to_book: String,
        to_book_relative: bool,
    },
}

impl<'a> Link<'a> {
    pub fn href(&'a self) -> &'a str {
        &self.href
    }

    pub fn repo_relative(&'a self) -> Option<&'a str> {
        self.href.strip_prefix('/')
    }

    pub fn interest(&self) -> ContentInterest {
        self.interest
    }

    pub fn span(&self) -> &LinkSpan {
        &self.span
    }

    pub fn state(&self) -> &Result<LinkState, LinkError> {
        &self.state
    }

    pub fn no_change(&mut self) {
        self.state = Ok(LinkState::BookLinkChecked)
    }

    pub fn book_link(&mut self, href: RelativeUrl) {
        self.state = Ok(LinkState::BookLinkUpdated);
        self.href = href.consume_with(CowStr::from);
    }

    pub fn permalink(&mut self, href: String) {
        self.state = Ok(LinkState::Permalink);
        self.href = href.into();
    }

    pub fn error(&mut self, error: LinkError) {
        self.state = Err(error)
    }

    fn changed(&self) -> Option<ContentInterest> {
        match self.state {
            Ok(LinkState::BookLinkUpdated) => Some(self.interest),
            Ok(LinkState::Permalink) => Some(self.interest),
            _ => None,
        }
    }

    fn to_markdown(&self) -> Tag<'a> {
        match self.interest {
            ContentInterest::Nav => Tag::Link {
                link_type: LinkType::Inline,
                dest_url: self.href.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
            ContentInterest::Raw => Tag::Image {
                link_type: LinkType::Inline,
                dest_url: self.href.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
        }
    }
}

impl PathError {
    pub fn at(self, cause: Url) -> LinkError {
        LinkError {
            error: self,
            cause,
            help: None,
        }
    }

    pub fn from_io(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::NotADirectory => Self::NotADirectory,
            err => Self::Inaccessible(err),
        }
    }
}

#[derive(Debug)]
pub struct LinkSlice<'a> {
    elem: Vec<LinkElem<'a>>,
    span: Range<usize>,
}

#[derive(Debug)]
enum LinkElem<'a> {
    Text(Event<'a>),
    Link {
        link: Box<Link<'a>>, // Box: large variant
    },
    Html {
        html: Vec<Event<'a>>,
        links: Vec<Link<'a>>,
    },
}

impl<'a> LinkSlice<'a> {
    pub fn links_mut(&mut self) -> impl Iterator<Item = &'_ mut Link<'a>> {
        self.elem.iter_mut().flat_map(|item| match item {
            LinkElem::Link { link } => std::slice::from_mut(link.as_mut()),
            LinkElem::Html { links, .. } => links,
            LinkElem::Text(..) => &mut [],
        })
    }

    fn links(&self) -> impl Iterator<Item = &'_ Link<'a>> {
        self.elem.iter().flat_map(|item| match item {
            LinkElem::Link { link } => std::slice::from_ref(link.as_ref()),
            LinkElem::Html { links, .. } => links,
            LinkElem::Text(..) => &[],
        })
    }

    pub fn emit(
        self,
    ) -> (
        Patch<'a, impl Iterator<Item = Event<'a>>>,
        Option<Range<usize>>,
    ) {
        let changed = self.links().any(|link| link.changed().is_some());
        if !changed {
            let len = (self.elem.iter())
                .map(|elem| match elem {
                    LinkElem::Text(..) => 1,
                    LinkElem::Link { .. } => 1,
                    LinkElem::Html { html, .. } => html.len(),
                })
                .sum();
            let mut iter = Vec::with_capacity(len);
            for elem in self.elem {
                match elem {
                    LinkElem::Text(event) => iter.push(event),
                    LinkElem::Link { link } => iter.push(Event::Start(link.to_markdown())),
                    LinkElem::Html { html, .. } => iter.extend(html),
                }
            }
            (Patch::Skip(iter.into_iter()), None)
        } else {
            let iter = EmitLinkSlice {
                iter: self.elem.into_iter(),
                span: self.span.clone(),
                opened: vec![],
            };
            (Patch::Link(iter), Some(self.span))
        }
    }

    fn text(&mut self, event: Event<'a>) {
        self.elem.push(LinkElem::Text(event));
    }

    fn nest(&mut self, link: LinkSlice<'a>) {
        self.elem.extend(link.elem);
    }
}

pub struct LinkReader<'a> {
    source: &'a str,
    opened: Option<LinkSlice<'a>>,
    html: Vec<Spanned<Event<'a>>>,
}

impl<'a> LinkReader<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            opened: None,
            html: vec![],
        }
    }

    pub fn read(
        &mut self,
        event: Option<Spanned<Event<'a>>>,
    ) -> Result<impl Iterator<Item = Patch<'a, LinkSlice<'a>>> + use<'a>> {
        use Event::*;

        let Self {
            source,
            opened,
            html,
        } = self;

        let event = match (event, html.last()) {
            (Some((event @ InlineHtml(..), span)), None | Some((InlineHtml(..), ..)))
            | (Some((event @ Html(..), span)), None | Some((Html(..), ..))) => {
                html.push((event, span));
                return Ok(None.into_iter().chain(None));
            }
            (event, _) => event,
        };

        let queued = match LinkSlice::html(std::mem::take(html)) {
            Ok(link) => {
                trace!(span = ?link.span, ">>> HTML");
                if let Some(opened) = opened {
                    opened.nest(link);
                    None
                } else {
                    Some(Patch::Link(link))
                }
            }
            Err(queued) => {
                if let Some(opened) = opened {
                    for (event, _) in queued {
                        opened.text(event);
                    }
                    None
                } else if queued.is_empty() {
                    None
                } else {
                    let queued = queued
                        .into_iter()
                        .map(|(event, _)| event)
                        .collect::<Vec<_>>();
                    Some(Patch::Skip(queued.into_iter()))
                }
            }
        };

        let patch = match event {
            Some((Start(tag @ (Tag::Link { .. } | Tag::Image { .. })), span)) => {
                let (interest, dest, title) = match tag {
                    Tag::Link {
                        dest_url, title, ..
                    } => (ContentInterest::Nav, dest_url, title),
                    Tag::Image {
                        dest_url, title, ..
                    } => (ContentInterest::Raw, dest_url, title),
                    _ => unreachable!(),
                };

                trace!(?span, ?interest, ">>>");
                trace!(opened = ?opened.as_ref().map(|link| &link.span));
                trace!(?dest, " │ ");
                trace!(?title, " │ ");

                let link = {
                    let link = Link {
                        state: Ok(LinkState::Unsupported),
                        span: match locate_text(source, &dest) {
                            Some(span) => LinkSpan::Exact(span),
                            None => LinkSpan::Fuzzy(span.clone()),
                        },
                        href: dest.clone(),
                        interest,
                        title,
                    };
                    LinkSlice {
                        elem: vec![LinkElem::Link {
                            link: Box::new(link),
                        }],
                        span,
                    }
                };

                if let Some(opened) = opened {
                    opened.nest(link)
                } else {
                    *opened = Some(link)
                }

                None
            }

            Some((event @ End(end @ (TagEnd::Link | TagEnd::Image)), span)) => {
                let mut link = match opened.take() {
                    Some(link) => link,
                    None => {
                        debug!(?span, "unexpected {end:?}");
                        bail!("markdown stream malformed at byte position {span:?}");
                    }
                };

                trace!(?span, "<<<");

                link.text(event);

                if span == link.span {
                    Some(Patch::Link(link))
                } else {
                    *opened = Some(link);
                    None
                }
            }

            Some((event, _)) => {
                if let Some(opened) = opened {
                    opened.text(event);
                    None
                } else {
                    Some(Patch::SkipOne(std::iter::once(event)))
                }
            }

            None => None,
        };

        Ok(queued.into_iter().chain(patch))
    }
}

impl<'a> LinkSlice<'a> {
    fn html(events: Vec<Spanned<Event<'a>>>) -> Result<Self, Vec<Spanned<Event<'a>>>> {
        if events.is_empty() {
            return Err(events);
        }

        let span = events[0].1.start..events[events.len() - 1].1.end;
        let origin = span.start;

        let mut links = vec![];

        let handler =
            Settings::new().append_element_content_handler(element_handler(|elem, name, value| {
                let span = elem.source_location().bytes();
                let span = span.start + origin..span.end + origin;
                let interest = match (name, &*elem.tag_name()) {
                    ("href", "a") => {
                        if elem.has_attribute("download") {
                            ContentInterest::Raw
                        } else {
                            ContentInterest::Nav
                        }
                    }
                    ("href", "link") => {
                        // https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Attributes/rel
                        match &*elem.get_attribute("rel").unwrap_or_default() {
                            "icon" | "stylesheet" | "preconnect" | "prefetch" | "preload"
                            | "modulepreload" | "dns-prefetch" => ContentInterest::Raw,
                            _ => ContentInterest::Nav,
                        }
                    }
                    ("href", _) => ContentInterest::Nav,
                    ("data", "object") => ContentInterest::Raw,
                    ("src", _) => ContentInterest::Raw,
                    (_, _) => ContentInterest::Raw,
                };
                let link = Link {
                    state: Ok(LinkState::Unsupported),
                    href: value.into(),
                    interest,
                    title: CowStr::Borrowed(""),
                    span: LinkSpan::Fuzzy(span),
                };
                links.push(link);
                None
            }));
        let mut wr = HtmlRewriter::new(handler, |_: &[u8]| ());
        events
            .iter()
            .try_for_each(|(chunk, _)| {
                let chunk = match chunk {
                    Event::InlineHtml(html) => html.as_bytes(),
                    Event::Html(html) => html.as_bytes(),
                    _ => unreachable!(),
                };
                wr.write(chunk)
            })
            .and_then(|_| wr.end())
            .expect("HTML lax parsing should be infallible");

        if links.is_empty() {
            return Err(events);
        }

        let html = events.into_iter().map(|(chunk, _)| chunk).collect();

        Ok(Self {
            elem: vec![LinkElem::Html { html, links }],
            span,
        })
    }
}

struct EmitLinkSlice<'a> {
    iter: std::vec::IntoIter<LinkElem<'a>>,
    span: Range<usize>,
    opened: Vec<ContentInterest>,
}

impl<'a> Iterator for EmitLinkSlice<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            match next {
                LinkElem::Link { link } => {
                    let span = &self.span;
                    match (link.changed(), self.opened.is_empty()) {
                        (Some(usage), top_level) => {
                            self.opened.push(usage);
                            let link = link.to_markdown();
                            trace!(?span, ?link, "{}", if top_level { ">" } else { ">>" });
                            return Some(Event::Start(link));
                        }
                        (None, false) => {
                            let link = link.to_markdown();
                            trace!(?span, ?link, ">│ skipped, link in link");
                            return Some(Event::Start(link));
                        }
                        (None, true) => {
                            trace!(?span, "│ skipped");
                            continue;
                        }
                    };
                }

                LinkElem::Html { html, links } => {
                    let mut links = links.into_iter();

                    let handler = Settings::new().append_element_content_handler(element_handler(
                        |_, _, _| {
                            let link = (links.next())
                                .expect("2nd parse should result in the same number of links");
                            if link.changed().is_some() {
                                Some(link.href.into())
                            } else {
                                None
                            }
                        },
                    ));

                    let mut output = String::new();
                    let mut writer = HtmlRewriter::new(handler, |chunk: &[u8]| {
                        let chunk = str::from_utf8(chunk)
                            .expect("`lol_html` guarantees that HTML is encoded");
                        output.push_str(chunk);
                    });

                    html.iter()
                        .try_for_each(|chunk| {
                            let chunk = match chunk {
                                Event::InlineHtml(chunk) => chunk.as_bytes(),
                                Event::Html(chunk) => chunk.as_bytes(),
                                _ => unreachable!(),
                            };
                            writer.write(chunk)
                        })
                        .and_then(|_| writer.end())
                        .expect("HTML lax parsing should be infallible");

                    let elem = match &html[0] {
                        Event::InlineHtml(_) => Event::InlineHtml(output.into()),
                        Event::Html(_) => Event::Html(output.into()),
                        _ => unreachable!(),
                    };

                    return Some(elem);
                }

                LinkElem::Text(elem) => {
                    match (elem, self.opened.last()) {
                        (elem @ Event::End(TagEnd::Link), Some(ContentInterest::Nav))
                        | (elem @ Event::End(TagEnd::Image), Some(ContentInterest::Raw)) => {
                            self.opened.pop();
                            let top_level = self.opened.is_empty();
                            trace!(?elem, "{}", if top_level { "<" } else { "<<" });
                            return Some(elem);
                        }
                        (Event::End(TagEnd::Link | TagEnd::Image), None) => {
                            trace!("│ skipped");
                            continue;
                        }
                        (elem, _) => {
                            let top_level = self.opened.len() == 1;
                            trace!(?elem, "{}", if top_level { "│" } else { " │" });
                            return Some(elem);
                        }
                    };
                }
            };
        }
        None
    }
}

fn element_handler<'cb>(
    mut cb: impl FnMut(&Element, &'static str, String) -> Option<String> + 'cb,
) -> (
    Cow<'static, lol_html::Selector>,
    lol_html::ElementContentHandlers<'cb, lol_html::LocalHandlerTypes>,
) {
    element!("  [href], [src], [data]", move |elem| {
        for name in ["href", "src", "data"] {
            if let Some(attr) = elem.get_attribute(name)
                && let Some(attr) = cb(elem, name, attr)
            {
                elem.set_attribute(name, &attr)
                    .expect("attribute name is valid");
            }
        }
        Ok(())
    })
}

impl Debug for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkError")
            .field("error", &self.error)
            .field("cause", &self.cause.show())
            .finish_non_exhaustive()
    }
}
