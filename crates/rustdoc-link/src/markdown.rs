use std::{borrow::Cow, fmt};

use anyhow::Result;
use pulldown_cmark::{BrokenLink, BrokenLinkCallback, CowStr, Event, Options, Parser};
use pulldown_cmark_to_cmark::cmark;
use tap::Pipe;

use crate::Spanned;

pub struct PatchStream<'s>(Vec<Cow<'s, str>>);

impl<'s> PatchStream<'s> {
    pub fn patch<'a, E, S>(source: &'s str, stream: S) -> Result<Self>
    where
        E: Iterator<Item = Event<'a>>,
        S: Iterator<Item = Spanned<E>>,
    {
        let mut output = vec![];
        let mut start = 0usize;

        for (events, span) in stream {
            let patch = String::new().pipe(|mut out| cmark(events, &mut out).and(Ok(out)))?;
            output.push(Cow::Borrowed(&source[start..span.start]));
            output.push(Cow::Owned(patch));
            start = span.end;
        }
        output.push(Cow::Borrowed(&source[start..]));

        Ok(Self(output))
    }
}

impl fmt::Display for PatchStream<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for p in self.0.iter() {
            fmt::Display::fmt(p, f)?;
        }
        Ok(())
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
