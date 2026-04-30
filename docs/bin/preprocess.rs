use std::ops::Range;

use anyhow::{Context, Result};

use mdbook_markdown::pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd};
use mdbookkit::{
    book::{BookHelper, book_from_stdin},
    markdown::PatchStream,
};

pub fn run() -> Result<()> {
    let (ctx, mut book) = book_from_stdin().context("Failed to read from mdBook")?;

    book.for_each_page_mut(|_, content| {
        let stream = Parser::new(content)
            .into_offset_iter()
            .scan(State::default(), |state, chunk| Some(state.scan(chunk)))
            .flat_map(|chunk| chunk?.replaced())
            .collect::<Vec<_>>();

        if !stream.is_empty() {
            *content = PatchStream::new(content, stream.into_iter()).into_string()?;
        }

        Ok::<_, anyhow::Error>(())
    })?;

    book.to_stdout(&ctx)
}

#[derive(Default)]
struct State {
    mermaid: Option<Range<usize>>,
}

impl State {
    fn scan<'a>(&mut self, (event, span): (Event<'a>, Range<usize>)) -> Option<Replace<'a>> {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(tag))) if &*tag == "mermaid" => {
                self.mermaid = Some(span);
            }
            Event::Text(text) => {
                if let Some(ref span) = self.mermaid {
                    let span = span.clone();
                    return Some(Replace::Mermaid { text, span });
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                self.mermaid = None;
            }
            _ => {}
        };
        None
    }
}

enum Replace<'a> {
    Mermaid {
        text: CowStr<'a>,
        span: Range<usize>,
    },
}

impl<'a> Replace<'a> {
    fn replaced(self) -> Option<(std::vec::IntoIter<Event<'a>>, Range<usize>)> {
        match self {
            Self::Mermaid { text, span } => {
                let repl = vec![
                    Event::Start(Tag::HtmlBlock),
                    Event::Html(CowStr::Borrowed("<pre class=\"mermaid\">")),
                    Event::Html(text),
                    Event::Html(CowStr::Borrowed("</pre>")),
                    Event::End(TagEnd::HtmlBlock),
                ]
                .into_iter();
                Some((repl, span))
            }
        }
    }
}
