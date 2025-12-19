use anyhow::{Context, Result};

use mdbook_markdown::pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd};
use mdbookkit::{
    book::{BookHelper, book_from_stdin},
    logging::ConsoleLogger,
    markdown::PatchStream,
};

fn preprocess() -> Result<()> {
    let (ctx, mut book) = book_from_stdin().context("failed to read from mdbook")?;

    book.for_each_text_mut(|_, content| {
        let stream = Parser::new(content)
            .into_offset_iter()
            .scan(None, |state, (event, span)| match event {
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(tag))) if &*tag == "mermaid" => {
                    *state = Some(span);
                    Some(None)
                }
                Event::Text(text) => {
                    if let Some(span) = state {
                        Some(Some((text, span.clone())))
                    } else {
                        Some(None)
                    }
                }
                Event::End(TagEnd::CodeBlock) => {
                    *state = None;
                    Some(None)
                }
                _ => Some(None),
            })
            .flat_map(|chunk| {
                let (text, span) = chunk?;
                let repl = vec![
                    Event::Start(Tag::HtmlBlock),
                    Event::Html(CowStr::Borrowed("<pre class=\"mermaid\">")),
                    Event::Html(text),
                    Event::Html(CowStr::Borrowed("</pre>")),
                    Event::End(TagEnd::HtmlBlock),
                ]
                .into_iter();
                Some((repl, span))
            })
            .collect::<Vec<_>>();

        if !stream.is_empty() {
            *content = PatchStream::new(content, stream.into_iter())
                .into_string()
                .unwrap();
        }
    });

    book.to_stdout(&ctx)
}

fn main() -> Result<()> {
    ConsoleLogger::install(env!("CARGO_PKG_NAME"));
    let Program { command } = clap::Parser::parse();
    match command {
        Command::Preprocess { command: None } => preprocess(),
        Command::Preprocess {
            command: Some(Preprocess::Supports { .. }),
        } => Ok(()),
        Command::Postprocess => Ok(()),
    }
}

#[derive(clap::Parser, Debug, Clone)]
struct Program {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    Preprocess {
        #[command(subcommand)]
        command: Option<Preprocess>,
    },
    Postprocess,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Preprocess {
    #[clap(hide = true)]
    Supports { renderer: String },
}
