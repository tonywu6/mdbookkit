use std::{fmt::Debug, ops::Range};

use bon::bon;
use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use tracing::trace;
use url::Url;

use mdbookkit::{error::Show, markdown::locate_text, url::RelativeUrl};

#[derive(Debug)]
pub struct Link<'a> {
    state: State<'a>,
    span: SourceSpan,
}

#[derive(Debug)]
pub struct State<'a> {
    state: Result<LinkState, LinkError>,
    href: CowStr<'a>,
    kind: ContentKind,
    title: CowStr<'a>,
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

impl<'a> State<'a> {
    pub fn no_change(&mut self) {
        self.state = Ok(LinkState::BookLinkChecked)
    }

    pub fn book_link(&mut self, href: RelativeUrl) {
        self.state = Ok(LinkState::BookLinkUpdated);
        self.href = href.consume_with(CowStr::from);
    }

    pub fn permalink(&mut self, href: Url) {
        self.state = Ok(LinkState::Permalink);
        self.href = String::from(href).into();
    }

    pub fn error(&mut self, error: LinkError) {
        self.state = Err(error)
    }

    fn should_emit(&self) -> Option<ContentKind> {
        match self.state {
            Ok(LinkState::BookLinkUpdated | LinkState::Permalink) => Some(self.kind),
            _ => None,
        }
    }

    fn emit(&self) -> Tag<'a> {
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

#[derive(Debug)]
pub struct SourceSpan {
    pub full: Range<usize>,
    pub link: Option<Range<usize>>,
}

impl<'a> Link<'a> {
    pub fn state(&self) -> &Result<LinkState, LinkError> {
        &self.state.state
    }

    pub fn state_mut(&mut self) -> &mut State<'a> {
        &mut self.state
    }

    pub fn repo_relative(&'a self) -> Option<&'a str> {
        self.state.href.strip_prefix('/')
    }

    pub fn href(&'a self) -> &'a str {
        &self.state.href
    }

    pub fn kind(&self) -> ContentKind {
        self.state.kind
    }

    pub fn span(&self) -> &SourceSpan {
        &self.span
    }
}

impl SourceSpan {
    pub fn any(&self) -> &Range<usize> {
        self.link.as_ref().unwrap_or(&self.full)
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

#[bon]
impl<'a> Link<'a> {
    #[builder]
    pub fn new(
        kind: ContentKind,
        href: CowStr<'a>,
        span: Range<usize>,
        source: &'a str,
        title: CowStr<'a>,
    ) -> Self {
        Self {
            span: SourceSpan {
                full: span,
                link: locate_text(source, &href),
            },
            state: State {
                state: Ok(LinkState::Unsupported),
                href,
                kind,
                title,
            },
        }
    }
}

pub struct LinkSpan<'a>(pub Vec<LinkText<'a>>);

pub enum LinkText<'a> {
    Text(Event<'a>),
    Link(Box<Link<'a>>), // large variant
}

impl<'a> LinkSpan<'a> {
    pub fn links_mut(&mut self) -> impl Iterator<Item = &'_ mut Link<'a>> {
        self.0.iter_mut().filter_map(|item| match item {
            LinkText::Link(link) => Some(link.as_mut()),
            LinkText::Text(..) => None,
        })
    }

    pub fn links(&self) -> impl Iterator<Item = &'_ Link<'a>> {
        self.0.iter().filter_map(|item| match item {
            LinkText::Link(link) => Some(link.as_ref()),
            LinkText::Text(..) => None,
        })
    }

    pub fn span(&self) -> &Range<usize> {
        match &self.0[0] {
            LinkText::Link(link) => &link.span.full,
            LinkText::Text(..) => unreachable!("first item in LinkSpan must be a Link"),
        }
    }
}

pub struct EmitLinkSpan<'a> {
    iter: std::slice::Iter<'a, LinkText<'a>>,
    opened: Vec<ContentKind>,
}

impl<'a> Iterator for EmitLinkSpan<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            match next {
                LinkText::Link(link) => {
                    let span = &link.span.full;
                    match (link.state.should_emit(), self.opened.is_empty()) {
                        (Some(usage), top_level) => {
                            self.opened.push(usage);
                            let link = link.state.emit();
                            trace!(?span, ?link, "{}", if top_level { ">" } else { ">>" });
                            return Some(Event::Start(link));
                        }
                        (None, false) => {
                            let link = link.state.emit();
                            trace!(?span, ?link, ">│ skipped, link in link");
                            return Some(Event::Start(link));
                        }
                        (None, true) => {
                            trace!(?span, "│ skipped");
                            continue;
                        }
                    };
                }
                LinkText::Text(text) => {
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
        let span = links.0.iter().find_map(|link| match &link {
            LinkText::Link(link) => {
                if link.state.should_emit().is_some() {
                    Some(link.span.full.clone())
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

impl Debug for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkError")
            .field("error", &self.error)
            .field("cause", &self.cause.show())
            .finish_non_exhaustive()
    }
}
