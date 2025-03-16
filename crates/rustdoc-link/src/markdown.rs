use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    ops::Range,
};

use anyhow::{bail, Context, Result};
use pulldown_cmark::{
    BrokenLink, BrokenLinkCallback, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd,
};
use pulldown_cmark_to_cmark::cmark;
use tap::Pipe;

#[derive(Debug)]
pub struct Page<'a> {
    source: &'a str,
    links: Vec<ParsedLink<'a>>,
}

#[derive(Debug)]
struct ParsedLink<'a> {
    dest_url: CowStr<'a>,
    title: CowStr<'a>,
    span: Range<usize>,
    inner: Vec<Event<'a>>,
}

impl<'a> Page<'a> {
    pub fn read<S>(source: &'a str, stream: S) -> Result<(Self, HashSet<String>)>
    where
        S: Iterator<Item = (Event<'a>, Range<usize>)>,
    {
        let mut items = HashSet::new();
        let mut links = Vec::new();
        let mut link: Option<ParsedLink> = None;

        for (event, span) in stream {
            if matches!(event, Event::End(TagEnd::Link)) {
                match link.take() {
                    Some(link) => {
                        if link.span == span {
                            links.push(link);
                            continue;
                        } else {
                            bail!("mismatching span, expected {:?}, found {span:?}", link.span)
                        }
                    }
                    None => bail!("unexpected `TagEnd::Link` at {span:?}"),
                }
            }

            let Event::Start(Tag::Link {
                dest_url, title, ..
            }) = event
            else {
                if let Some(link) = link.as_mut() {
                    link.inner.push(event);
                }
                continue;
            };

            if link.is_some() {
                bail!("unexpected `Tag::Link` in `Tag::Link`")
            }

            items.insert(dest_url.to_string());

            link = Some(ParsedLink {
                dest_url,
                title,
                span,
                inner: vec![],
            });
        }

        Ok((Page { source, links }, items))
    }

    pub fn emit<L>(&self, mut link_getter: L) -> Result<String>
    where
        L: FnMut(&str) -> Option<&'a str>,
    {
        let Self { source, links } = self;

        let mut output = String::with_capacity(source.len());
        let mut start = 0usize;

        for ParsedLink {
            dest_url,
            title,
            span,
            inner,
        } in links
        {
            output.push_str(&source[start..span.start]);

            if let Some(dest_url) = link_getter(dest_url) {
                let link = Tag::Link {
                    link_type: LinkType::Inline,
                    dest_url: dest_url.into(),
                    title: title.clone(),
                    id: CowStr::Borrowed(""),
                };

                let stream = std::iter::once(Event::Start(link))
                    .chain(inner.iter().cloned())
                    .chain(std::iter::once(Event::End(TagEnd::Link)));

                String::new()
                    .pipe(|mut wr| cmark(stream, &mut wr).and(Ok(wr)))?
                    .pipe(|out| output.push_str(&out));
            } else {
                output.push_str(&source[span.clone()]);
            }

            start = span.end;
        }

        if start < source.len() {
            output.push_str(&source[start..]);
        }

        Ok(output)
    }
}

impl<'a, K: Eq + Hash> Pages<'a, K> {
    pub fn read<S>(&mut self, key: K, source: &'a str, stream: S) -> Result<HashSet<String>>
    where
        S: Iterator<Item = (Event<'a>, Range<usize>)>,
    {
        let (page, items) = Page::read(source, stream)?;
        self.pages.insert(key, page);
        Ok(items)
    }

    pub fn emit<Q, L>(&self, key: &Q, links: L) -> Result<String>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + Debug + ?Sized,
        L: FnMut(&str) -> Option<&'a str>,
    {
        let page = self
            .pages
            .get(key)
            .with_context(|| format!("no such document {key:?}"))?;

        page.emit(links)
    }
}

#[derive(Debug, Default)]
pub struct Pages<'a, K> {
    pages: HashMap<K, Page<'a>>,
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
            let title = inner.clone();
            Some((inner, title))
        }
    }
}

/// <https://github.com/rust-lang/mdBook/blob/v0.4.47/src/utils/mod.rs#L197-L208>
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
