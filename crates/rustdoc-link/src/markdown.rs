use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

use anyhow::{Context, Result};
use pulldown_cmark::{
    BrokenLink, BrokenLinkCallback, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd,
};
use pulldown_cmark_to_cmark::cmark;
use tap::Pipe;

#[derive(Debug, Default)]
pub struct Pages<'a, K> {
    pages: HashMap<K, Vec<Event<'a>>>,
}

impl<'a, K: Eq + Hash> Pages<'a, K> {
    pub fn read<S>(&mut self, key: K, stream: S) -> HashSet<String>
    where
        S: Iterator<Item = Event<'a>>,
    {
        let mut items = HashSet::new();
        let buffer = stream
            .inspect(|event| {
                if let Event::Start(Tag::Link { dest_url, .. }) = &event {
                    items.insert(dest_url.to_string());
                }
            })
            .collect::<Vec<_>>();
        self.pages.insert(key, buffer);
        items
    }

    pub fn emit<Q, L>(&self, key: &Q, mut links: L) -> Result<String>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + Debug + ?Sized,
        L: FnMut(&str) -> Option<&'a str>,
    {
        let buffer = self
            .pages
            .get(key)
            .with_context(|| format!("no such document {key:?}"))?;

        enum Suffix<'a> {
            Collapsed,
            Shortcut,
            Reference(CowStr<'a>),
        }

        impl Suffix<'_> {
            // how is this 'static ?
            fn as_str(&self) -> CowStr<'static> {
                match self {
                    Self::Collapsed => "][]".into(),
                    Self::Shortcut => "]".into(),
                    Self::Reference(id) => format!("][{id}]").into(),
                }
            }
        }

        let stream = buffer
            .iter()
            .cloned()
            .scan(Option::<Suffix>::None, |suffix, mut event| {
                if let Event::Start(Tag::Link {
                    dest_url,
                    id,
                    link_type,
                    ..
                }) = &mut event
                {
                    if let Some(found) = links(dest_url) {
                        *dest_url = found.to_owned().into();
                        Some(event)
                    } else if matches!(
                        link_type,
                        LinkType::CollapsedUnknown
                            | LinkType::ReferenceUnknown
                            | LinkType::ShortcutUnknown
                    ) {
                        // don't emit unresolved "broken" links as links
                        *suffix = match link_type {
                            LinkType::ShortcutUnknown => Some(Suffix::Shortcut),
                            LinkType::CollapsedUnknown => Some(Suffix::Collapsed),
                            LinkType::ReferenceUnknown => Some(Suffix::Reference(id.clone())),
                            _ => unreachable!(),
                        };
                        Some(Event::Text(CowStr::Borrowed("[")))
                    } else {
                        Some(event)
                    }
                } else if matches!(event, Event::End(TagEnd::Link)) {
                    if let Some(suffix) = suffix.take() {
                        // a link was dropped, patch corresponding TagEnd::Link
                        Some(Event::Text(suffix.as_str()))
                    } else {
                        Some(event)
                    }
                } else {
                    Some(event)
                }
                .pipe(Some)
            })
            .flatten();

        String::new()
            .pipe(|mut wr| cmark(stream, &mut wr).and(Ok(wr)))?
            .pipe(Ok)
    }
}

pub fn markdown_parser(text: &str, smart_punctuation: bool) -> MarkdownStream<'_> {
    Parser::new_with_broken_link_callback(text, options(smart_punctuation), Some(BrokenLinks))
}

pub type MarkdownStream<'a> = Parser<'a, BrokenLinks>;

pub struct BrokenLinks;

impl<'input> BrokenLinkCallback<'input> for BrokenLinks {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        let inner = if let CowStr::Borrowed(inner) = link.reference {
            let parse = markdown_parser(inner, false);

            let inner = parse
                .filter_map(|event| match event {
                    Event::Text(inner) => Some(inner),
                    Event::Code(inner) => Some(inner),
                    _ => None,
                })
                .collect::<Vec<_>>();

            if inner.len() == 1 {
                inner.into_iter().next().unwrap()
            } else {
                inner
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Box<str>>()
                    .pipe(CowStr::Boxed)
            }
        } else {
            link.reference.clone()
        };
        if inner.is_empty() {
            None
        } else {
            Some((inner, link.reference))
        }
    }
}

fn options(smart_punctuation: bool) -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    if smart_punctuation {
        opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    }
    opts
}
