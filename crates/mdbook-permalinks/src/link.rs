use std::{fmt::Debug, ops::Range};

use bon::bon;
use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use tracing::trace;
use url::Url;

use mdbookkit::{error::Show, markdown::locate_text, url::RelativeUrl};

#[derive(Debug)]
pub struct Link<'a> {
    state: Result<LinkState, LinkError>,
    href: CowStr<'a>,
    kind: ContentKind,
    title: CowStr<'a>,
    span: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Web,
    Raw,
}

#[derive(Debug, Clone, Copy)]
pub enum LinkState {
    Unsupported,
    BookLinkChecked,
    BookLinkUpdated,
    Permalink,
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

    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    pub fn span(&self) -> &Range<usize> {
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

    fn changed(&self) -> Option<ContentKind> {
        match self.state {
            Ok(LinkState::BookLinkUpdated | LinkState::Permalink) => Some(self.kind),
            _ => None,
        }
    }

    fn to_markdown(&self) -> Tag<'a> {
        match self.kind {
            ContentKind::Web => Tag::Link {
                link_type: LinkType::Inline,
                dest_url: self.href.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
            ContentKind::Raw => Tag::Image {
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

pub struct LinkSpan<'a> {
    elem: Vec<LinkElem<'a>>,
    span: Range<usize>,
}

enum LinkElem<'a> {
    Text(Event<'a>),
    Link {
        link: Box<Link<'a>>, // Box: large variant
    },
}

impl<'a> LinkSpan<'a> {
    pub fn text(&mut self, event: Event<'a>) {
        self.elem.push(LinkElem::Text(event));
    }

    pub fn nested(&mut self, link: LinkSpan<'a>) {
        self.elem.extend(link.elem);
    }

    pub fn links_mut(&mut self) -> impl Iterator<Item = &'_ mut Link<'a>> {
        self.elem.iter_mut().flat_map(|item| match item {
            LinkElem::Link { link } => std::slice::from_mut(link.as_mut()),
            LinkElem::Text(..) => &mut [],
        })
    }

    pub fn links(&self) -> impl Iterator<Item = &'_ Link<'a>> {
        self.elem.iter().flat_map(|item| match item {
            LinkElem::Link { link } => std::slice::from_ref(link.as_ref()),
            LinkElem::Text(..) => &[],
        })
    }

    pub fn span(&self) -> &Range<usize> {
        &self.span
    }

    pub fn emit(&'a self) -> Option<(impl Iterator<Item = Event<'a>>, Range<usize>)> {
        EmitLinkSpan::new(self)
    }
}

#[bon]
impl<'a> LinkSpan<'a> {
    #[builder(finish_fn = open)]
    pub fn markdown(
        href: CowStr<'a>,
        kind: ContentKind,
        span: Range<usize>,
        source: &'a str,
        title: CowStr<'a>,
    ) -> Self {
        let link = Link {
            span: locate_text(source, &href).unwrap_or_else(|| span.clone()),
            state: Ok(LinkState::Unsupported),
            href,
            kind,
            title,
        };
        let elem = LinkElem::Link {
            link: Box::new(link),
        };
        Self {
            elem: vec![elem],
            span,
        }
    }
}

struct EmitLinkSpan<'a> {
    iter: std::slice::Iter<'a, LinkElem<'a>>,
    span: &'a Range<usize>,
    opened: Vec<ContentKind>,
}

impl<'a> Iterator for EmitLinkSpan<'a> {
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

                LinkElem::Text(text) => {
                    match (text, self.opened.last()) {
                        (Event::End(TagEnd::Link), Some(ContentKind::Web))
                        | (Event::End(TagEnd::Image), Some(ContentKind::Raw)) => {
                            self.opened.pop();
                            let top_level = self.opened.is_empty();
                            trace!(?text, "{}", if top_level { "<" } else { "<<" });
                            return Some(text.clone());
                        }
                        (Event::End(TagEnd::Link | TagEnd::Image), None) => {
                            trace!("│ skipped");
                            continue;
                        }
                        _ => {
                            let top_level = self.opened.len() == 1;
                            trace!(?text, "{}", if top_level { "│" } else { " │" });
                            return Some(text.clone());
                        }
                    };
                }
            };
        }
        None
    }
}

impl<'a> EmitLinkSpan<'a> {
    pub fn new(links: &'a LinkSpan<'a>) -> Option<(Self, Range<usize>)> {
        let changed = links.elem.iter().any(|elem| match elem {
            LinkElem::Link { link } => link.changed().is_some(),
            _ => false,
        });
        if !changed {
            return None;
        }
        let iter = EmitLinkSpan {
            iter: links.elem.iter(),
            span: &links.span,
            opened: vec![],
        };
        let span = links.span.clone();
        Some((iter, span))
    }
}

impl Debug for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkError")
            .field("error", &self.error)
            .field("cause", &self.cause.show())
            .finish_non_exhaustive()
    }
}
