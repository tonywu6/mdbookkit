use std::{
    collections::{HashMap, HashSet},
    io::Write,
    ops::Range,
    process::{Command, Stdio},
};

use anyhow::{Context, Result};

use mdbook_markdown::pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd};

use mdbookkit::{
    book::{BookHelper, book_from_stdin},
    config::{ConfigExampleErrors, ConfigExampleInputs},
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, SourceCode,
        annotate_snippets::AnnotationKind,
    },
    error::OnWarning,
    markdown::{PatchStream, Spanned},
};

pub fn run() -> Result<()> {
    let (ctx, mut book) = book_from_stdin().context("Failed to read from mdBook")?;

    let mut global = BookState::default();

    book.for_each_page_mut(|path, content| {
        let path = path.display().to_string();
        global.insert(path.clone(), content.clone());

        let stream = Parser::new(content)
            .into_offset_iter()
            .scan(PageState::default(), |state, chunk| Some(state.scan(chunk)))
            .flat_map(|chunk| global.consume(path.clone(), chunk?))
            .collect::<Vec<_>>();

        if !stream.is_empty() {
            *content = PatchStream::new(content, stream.into_iter()).into_string()?;
        }

        Ok::<_, anyhow::Error>(())
    })?;

    for (package, examples) in global.config_examples.iter() {
        let mut proc = Command::new(env!("CARGO"))
            .args(["run", "--package", package])
            .args(["--", "validate-config"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        proc.stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&examples)?)?;
        let result = proc.wait_with_output()?;
        if !result.status.success() {
            let result = serde_json::from_slice::<ConfigExampleErrors>(&result.stdout)?;
            for (path, errors) in result.0 {
                for (error, span) in errors {
                    let issue = IssueReport::level(IssueLevel::Warning)
                        .title("invalid config snippet")
                        .annotations(vec![
                            Highlight::span(span).kind(AnnotationKind::Context).build(),
                        ])
                        .notes(vec![Note::note(error)])
                        .build();
                    IssueReporter {
                        issues: vec![issue],
                        source: global.source(&path),
                    }
                    .emit();
                }
            }
        }
    }

    OnWarning::FailInCi.check()?;

    book.to_stdout(&ctx)
}

#[derive(Debug, Default)]
struct BookState {
    sources: HashMap<String, String>,
    config_examples: HashMap<&'static str, ConfigExampleInputs>,
}

#[derive(Default)]
struct PageState {
    mermaid: Option<Range<usize>>,
    config_example: Option<Spanned<&'static str>>,
}

enum Place<'a> {
    Mermaid(CowStr<'a>),
    ConfigExample {
        package: &'static str,
        text: CowStr<'a>,
    },
}

impl BookState {
    fn insert(&mut self, path: String, text: String) {
        self.sources.insert(path, text);
    }

    fn source<'a>(&'a self, path: &'a str) -> SourceCode<'a> {
        let source = self.sources.get(path).unwrap();
        SourceCode {
            source_code: source,
            source_path: path.into(),
        }
    }

    fn consume<'a>(
        &mut self,
        path: String,
        (place, span): Spanned<Place<'a>>,
    ) -> Option<Spanned<std::vec::IntoIter<Event<'a>>>> {
        match place {
            Place::Mermaid(text) => {
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
            Place::ConfigExample { package, text } => {
                self.config_examples
                    .entry(package)
                    .or_default()
                    .0
                    .entry(path)
                    .or_default()
                    .push((text.into_string(), span));
                None
            }
        }
    }
}

impl PageState {
    fn scan<'a>(&mut self, (event, span): Spanned<Event<'a>>) -> Option<Spanned<Place<'a>>> {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(tag))) => {
                let tags = tag.split(' ').map(|tag| tag.trim()).collect::<HashSet<_>>();
                if tags.contains("mermaid") {
                    self.mermaid = Some(span)
                } else if tags.contains("toml") {
                    if tags.contains("config-example-rustdoc-links") {
                        self.config_example = Some(("mdbook-rustdoc-links", span))
                    } else if tags.contains("config-example-permalinks") {
                        self.config_example = Some(("mdbook-permalinks", span))
                    }
                }
            }
            Event::Text(text) => {
                if let Some(span) = self.mermaid.take() {
                    return Some((Place::Mermaid(text), span));
                }
                if let Some((package, span)) = self.config_example.take() {
                    return Some((Place::ConfigExample { package, text }, span));
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
