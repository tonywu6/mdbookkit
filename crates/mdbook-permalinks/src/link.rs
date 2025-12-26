use std::{fmt::Debug, ops::Range};

use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use tracing::{debug, trace};
use url::Url;

#[derive(Debug, Default, Clone, thiserror::Error)]
pub enum LinkStatus {
    #[default]
    #[error("link ignored")]
    Ignored,

    #[error("linking to book page or file")]
    Unchanged,
    #[error("linking to book page or file, rewritten as paths")]
    Rewritten,
    #[error("link converted to permalink")]
    Permalink,

    #[error("link inaccessible")]
    Unreachable(Vec<(Url, PathStatus)>),

    #[error("error encountered: {0}")]
    Error(String),
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum PathStatus {
    #[error("does not exist")]
    Unreachable,
    #[error("is ignored by git")]
    Ignored,
    #[error("is not in repo")]
    NotInRepo,
    #[error("is not in SUMMARY.md")]
    NotInBook,
}

pub struct LinkSpan<'a>(pub Vec<LinkText<'a>>);

pub enum LinkText<'a> {
    Text(Event<'a>),
    Link(RelativeLink<'a>),
}

pub struct RelativeLink<'a> {
    pub status: LinkStatus,
    pub span: Range<usize>,
    pub link: CowStr<'a>,
    pub hint: ContentHint,
    pub title: CowStr<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentHint {
    Tree,
    Raw,
}

impl<'a> LinkSpan<'a> {
    pub fn links_mut(&mut self) -> impl Iterator<Item = &'_ mut RelativeLink<'a>> {
        self.0.iter_mut().filter_map(|item| match item {
            LinkText::Link(link) => Some(link),
            LinkText::Text(..) => None,
        })
    }

    pub fn links(&self) -> impl Iterator<Item = &'_ RelativeLink<'a>> {
        self.0.iter().filter_map(|item| match item {
            LinkText::Link(link) => Some(link),
            LinkText::Text(..) => None,
        })
    }

    pub fn span(&self) -> &Range<usize> {
        match &self.0[0] {
            LinkText::Link(link) => &link.span,
            LinkText::Text(..) => unreachable!("first item in LinkSpan must be a Link"),
        }
    }
}

impl<'a> RelativeLink<'a> {
    #[inline]
    pub fn rewritten(&mut self, link: impl Into<CowStr<'a>>) {
        self.status = LinkStatus::Rewritten;
        self.update(link);
    }

    #[inline]
    pub fn permalink(&mut self, link: impl Into<CowStr<'a>>) {
        self.status = LinkStatus::Permalink;
        self.update(link);
    }

    #[inline]
    fn update(&mut self, link: impl Into<CowStr<'a>>) {
        let old = &*self.link.clone();
        self.link = link.into();
        debug!(status = ?self.status, ?old, new = ?&*self.link);
    }

    #[inline]
    pub fn unchanged(&mut self) {
        self.status = LinkStatus::Unchanged;
        debug!(status = ?self.status, link = ?&*self.link);
    }

    #[inline]
    pub fn unreachable(&mut self, errors: Vec<(Url, PathStatus)>) {
        self.status = LinkStatus::Unreachable(errors);
        debug!(status = ?self.status, link = ?&*self.link);
    }

    fn emit(&self) -> Tag<'a> {
        match self.hint {
            ContentHint::Tree => Tag::Link {
                link_type: LinkType::Inline,
                dest_url: self.link.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
            ContentHint::Raw => Tag::Image {
                link_type: LinkType::Inline,
                dest_url: self.link.clone(),
                title: self.title.clone(),
                id: CowStr::Borrowed(""),
            },
        }
    }

    fn will_emit(&self) -> Option<ContentHint> {
        match self.status {
            LinkStatus::Ignored => None,
            LinkStatus::Unchanged => None,
            LinkStatus::Rewritten => Some(self.hint),
            LinkStatus::Permalink => Some(self.hint),
            LinkStatus::Unreachable(_) => None,
            LinkStatus::Error(_) => None,
        }
    }
}

pub struct EmitLinkSpan<'a> {
    iter: std::slice::Iter<'a, LinkText<'a>>,
    opened: Vec<ContentHint>,
}

impl<'a> Iterator for EmitLinkSpan<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for next in self.iter.by_ref() {
            match next {
                LinkText::Link(link) => {
                    let span = &link.span;
                    match (link.will_emit(), self.opened.is_empty()) {
                        (Some(usage), top_level) => {
                            self.opened.push(usage);
                            let link = link.emit();
                            trace!(?span, ?link, "{}", if top_level { ">" } else { ">>" });
                            return Some(Event::Start(link));
                        }
                        (None, false) => {
                            let link = link.emit();
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
                        (Event::End(TagEnd::Link), Some(ContentHint::Tree))
                        | (Event::End(TagEnd::Image), Some(ContentHint::Raw)) => {
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
