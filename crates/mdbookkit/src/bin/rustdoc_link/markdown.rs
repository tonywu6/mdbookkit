use pulldown_cmark::{BrokenLink, BrokenLinkCallback, CowStr, Event, Parser};
use tap::Pipe;

use crate::markdown::markdown_options;

pub fn stream(text: &str, smart_punctuation: bool) -> MarkdownStream<'_> {
    Parser::new_with_broken_link_callback(
        text,
        markdown_options(smart_punctuation),
        Some(BrokenLinks),
    )
}

pub type MarkdownStream<'a> = Parser<'a, BrokenLinks>;

pub struct BrokenLinks;

impl<'input> BrokenLinkCallback<'input> for BrokenLinks {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        let inner = if let CowStr::Borrowed(inner) = link.reference {
            let parse = stream(inner, false);

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
