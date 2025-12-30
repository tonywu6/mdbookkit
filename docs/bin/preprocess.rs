use std::{ops::Range, process::Stdio, sync::LazyLock};

use anyhow::{Context, Result};

use mdbook_markdown::pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd};
use tap::Pipe;

use mdbookkit::{
    book::{BookHelper, book_from_stdin},
    emit_error,
    error::ExitProcess,
    markdown::PatchStream,
};

pub fn run() -> Result<()> {
    let (ctx, mut book) = book_from_stdin().context("Failed to read from mdBook")?;

    #[derive(Default)]
    struct State {
        mermaid: Option<Range<usize>>,
        ra_version: Option<Range<usize>>,
        describe: Option<Range<usize>>,
    }

    enum Replace<'a> {
        Mermaid {
            text: CowStr<'a>,
            span: Range<usize>,
        },
        RustAnalyzerVersion {
            span: Range<usize>,
        },
        Describe {
            package: &'static str,
            span: Range<usize>,
        },
    }

    book.for_each_text_mut(|_, content| {
        let stream = Parser::new(content)
            .into_offset_iter()
            .scan(State::default(), |state, (event, span)| {
                match event {
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(tag)))
                        if &*tag == "mermaid" =>
                    {
                        state.mermaid = Some(span);
                    }
                    Event::Text(text) => {
                        if let Some(ref span) = state.mermaid {
                            let span = span.clone();
                            return Some(Some(Replace::Mermaid { text, span }));
                        }
                    }
                    Event::End(TagEnd::CodeBlock) => {
                        state.mermaid = None;
                    }
                    Event::InlineHtml(tag) => match &*tag {
                        "<ra-version>" => state.ra_version = Some(span),
                        "</ra-version>" => {
                            if let Some(start) = state.ra_version.take() {
                                let span = start.start..span.end;
                                return Some(Some(Replace::RustAnalyzerVersion { span }));
                            }
                        }
                        "<rustdoc-links-options>" => state.describe = Some(span),
                        "</rustdoc-links-options>" => {
                            if let Some(start) = state.describe.take() {
                                let span = start.start..span.end;
                                let package = "mdbook-rustdoc-links";
                                return Some(Some(Replace::Describe { package, span }));
                            }
                        }
                        "<permalinks-options>" => state.describe = Some(span),
                        "</permalinks-options>" => {
                            if let Some(start) = state.describe.take() {
                                let span = start.start..span.end;
                                let package = "mdbook-permalinks";
                                return Some(Some(Replace::Describe { package, span }));
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                Some(None)
            })
            .flat_map(|chunk| match chunk? {
                Replace::Mermaid { text, span } => {
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
                Replace::RustAnalyzerVersion { span } => {
                    static RA_VERSION: LazyLock<String> = LazyLock::new(|| {
                        std::process::Command::new(env!("CARGO"))
                            .args(["xtask", "rust-analyzer", "version"])
                            .stdout(Stdio::piped())
                            .output()
                            .context("failed to run xtask rust-analyzer")
                            .exit(emit_error!())
                            .stdout
                            .pipe(String::from_utf8)
                            .context("failed to parse version")
                            .exit(emit_error!())
                    });
                    let repl = vec![Event::Code(RA_VERSION.clone().into())].into_iter();
                    Some((repl, span))
                }
                Replace::Describe { package, span } => {
                    let described = std::process::Command::new(env!("CARGO"))
                        .args([
                            "run",
                            "--package",
                            package,
                            "--features",
                            "_testing",
                            "--",
                            "describe",
                        ])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::inherit())
                        .output()
                        .with_context(|| format!("failed to describe {package}"))
                        .exit(emit_error!())
                        .stdout
                        .pipe(String::from_utf8)
                        .context("failed to parse version")
                        .exit(emit_error!());
                    let repl = vec![Event::Html(described.into())].into_iter();
                    Some((repl, span))
                }
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
