use std::{fmt::Debug, ops::Range};

use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};

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

pub struct LinkSpan<'a>(pub Vec<LinkText<'a>>);

pub enum LinkText<'a> {
    Text(Event<'a>),
    Link(RelativeLink<'a>),
}

pub struct RelativeLink<'a> {
    pub status: LinkStatus,
    pub span: Range<usize>,
    pub link: CowStr<'a>,
    pub usage: LinkUsage,
    pub title: CowStr<'a>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum LinkUsage {
    Link,
    Image,
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

pub struct EmitLinkSpan<'a> {
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

impl Debug for LinkUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link => f.write_str("link"),
            Self::Image => f.write_str("image"),
        }
    }
}
