use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io::Write as _,
    ops::Range,
    process::{Command, Stdio},
    sync::LazyLock,
};

use anyhow::{Context, Result, anyhow};
use camino::Utf8PathBuf;
use heck::ToSnakeCase;
use mdbook_markdown::pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd};
use minijinja::{Environment, UndefinedBehavior};

use mdbookkit::{
    book::{PreprocessorHelper, book_from_stdin},
    config::{ConfigExampleErrors, ConfigExampleInputs},
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, SourceCode,
        annotate_snippets::AnnotationKind,
    },
    emit, emit_error, emit_warning,
    env::locate_project,
    error::{ExpectFmt, FailOnWarnings, WithPathDebug},
    markdown::{PatchStream, Spanned},
    url::{ExpectUrl, UrlFromPath, UrlUtil},
};

pub fn run() -> Result<(), ()> {
    let (ctx, mut book) = book_from_stdin()
        .context("failed to read from mdBook")
        .or_else(emit_error!())?;

    let page_dir = ctx.page_dir().or_else(emit_error!())?.dir_to_url();

    let jinja = {
        let mut jinja = Environment::new();

        jinja.set_path_join_callback({
            let page_dir = page_dir.clone();
            move |name, referrer| {
                if let Some(name) = name.strip_prefix('/') {
                    CARGO_WORKSPACE.join(name).into_string().into()
                } else {
                    page_dir
                        .join(referrer)
                        .expect_url()
                        .join(name)
                        .expect_url()
                        .to_file_path()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                        .into()
                }
            }
        });
        jinja.set_loader(|name| {
            match std::fs::read_to_string(name)
                .with_path_debug(name)
                .context("could not load template")
            {
                Ok(template) => Ok(Some(template)),
                Err(err) => Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("{err:?}"),
                )),
            }
        });

        jinja.set_undefined_behavior(UndefinedBehavior::Strict);
        jinja.add_filter("snake_case", |text: String| text.to_snake_case());
        jinja
    };

    let mut state = BookState::default();

    ctx.for_each_page_mut(&mut book, |path, content| {
        let name = page_dir.print_relative(&path).to_string();

        if let Ok(rendered) = jinja
            .render_named_str(&name, content, ())
            .with_path_debug(&name)
            .context("failed to render page content using minijinja")
            .or_else(emit_warning!())
        {
            *content = rendered;
        }

        let stream = Parser::new(content)
            .into_offset_iter()
            .scan(PageState::default(), |inner, chunk| {
                Some(match inner.scan(chunk) {
                    Some((Ok(elem), span)) => match state.consume(&name, (elem, span.clone())) {
                        Ok(chunk) => chunk,
                        Err(err) => {
                            state.report(&name, error_report(&err, span));
                            None
                        }
                    },
                    Some((Err(err), span)) => {
                        state.report(&name, error_report(&err, span));
                        None
                    }
                    None => None,
                })
            })
            .flatten()
            .collect::<Vec<_>>();

        if !stream.is_empty()
            && let Ok(rendered) = PatchStream::new(content, stream.into_iter())
                .into_string()
                .with_path_debug(&name)
                .context("failed to patch page content")
                .or_else(emit_warning!())
        {
            *content = rendered;
        }

        Ok(())
    })?;

    state.validate_config().or_else(emit_error!())?;

    ctx.for_each_page(&book, |path, content| {
        let name = page_dir.print_relative(&path).to_string();
        if let Some(issues) = state.issues.remove(&name) {
            IssueReporter {
                issues,
                source: SourceCode {
                    source_code: content,
                    source_path: name.into(),
                },
            }
            .emit(emit!());
        }
        Ok(())
    })?;

    FailOnWarnings::InPipelines.check().or_else(emit_error!())?;

    ctx.print(book).or_else(emit_error!())
}

static CARGO_WORKSPACE: LazyLock<Utf8PathBuf> =
    LazyLock::new(|| locate_project(None).or_else(emit_error!()).unwrap());

fn error_report(error: &anyhow::Error, span: Range<usize>) -> IssueReport<'static> {
    IssueReport::level(IssueLevel::Error)
        .title(format!("{error}"))
        .annotations(vec![
            Highlight::span(span).kind(AnnotationKind::Context).build(),
        ])
        .notes(if error.source().is_some() {
            vec![Note::note(format!("{error:?}"))]
        } else {
            vec![]
        })
        .build()
}

#[derive(Debug, Default)]
struct BookState {
    config_examples: HashMap<&'static str, ConfigExampleInputs>,
    issues: HashMap<String, Vec<IssueReport<'static>>>,
}

#[derive(Default)]
struct PageState<'a> {
    mermaid: Option<Spanned<TextContent<'a>>>,
    config_snippet: Option<Spanned<ConfigSnippet<'a>>>,
}

type TextContent<'a> = Vec<CowStr<'a>>;

enum ConfigSnippet<'a> {
    Full(TextContent<'a>),
    Diff(TextContent<'a>),
}

enum Element<'a> {
    Mermaid(TextContent<'a>),
    ConfigSnippet(ConfigSnippet<'a>),
}

impl BookState {
    fn report(&mut self, path: &str, issue: IssueReport<'static>) {
        match self.issues.get_mut(path) {
            None => self.issues.entry(path.to_owned()).or_default(),
            Some(issues) => issues,
        }
        .push(issue);
    }

    fn consume<'a>(
        &mut self,
        path: &str,
        (elem, span): Spanned<Element<'a>>,
    ) -> Result<Option<Spanned<Vec<Event<'a>>>>> {
        match elem {
            Element::Mermaid(text) => {
                let repl = [
                    Event::Start(Tag::HtmlBlock),
                    Event::Html(CowStr::Borrowed("<pre class=\"mermaid\">")),
                ]
                .into_iter()
                .chain(text.into_iter().map(Event::Html))
                .chain([
                    Event::Html(CowStr::Borrowed("</pre>")),
                    Event::End(TagEnd::HtmlBlock),
                ])
                .collect();
                Ok(Some((repl, span)))
            }

            Element::ConfigSnippet(elem) => {
                let package = if path.starts_with("rustdoc-links") {
                    "mdbook-rustdoc-links"
                } else if path.starts_with("permalinks") {
                    "mdbook-permalinks"
                } else {
                    panic!("config examples are not available in {path:?}")
                };
                let (text, span) = match elem {
                    ConfigSnippet::Full(text) => (text.join(""), span),
                    ConfigSnippet::Diff(text) => {
                        let text = (text.iter())
                            .flat_map(|text| text.lines())
                            .filter_map(|line| {
                                if let Some(line) = line.strip_prefix("  ") {
                                    Some(line)
                                } else if let Some(line) = line.strip_prefix("+ ") {
                                    Some(line)
                                } else if line.starts_with("- ") {
                                    None
                                } else {
                                    Some(line)
                                }
                            })
                            .fold(String::new(), |mut out, line| {
                                writeln!(out, "{line}").expect_fmt();
                                out
                            });
                        (text, span)
                    }
                };
                let examples = &mut self.config_examples.entry(package).or_default().0;
                match examples.get_mut(path) {
                    None => examples.entry(path.to_owned()).or_default(),
                    Some(examples) => examples,
                }
                .push((text, span));
                Ok(None)
            }
        }
    }

    fn validate_config(&mut self) -> Result<()> {
        for (package, examples) in std::mem::take(&mut self.config_examples) {
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
                        self.report(&path, error_report(&anyhow!(error), span));
                    }
                }
            }
        }

        Ok(())
    }
}

impl<'a> PageState<'a> {
    fn scan(&mut self, (event, span): Spanned<Event<'a>>) -> Option<Spanned<Result<Element<'a>>>> {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(tag))) => {
                let tags = tag.split(' ').map(|tag| tag.trim()).collect::<HashSet<_>>();
                if tags.contains("mermaid") {
                    self.mermaid = Some((vec![], span))
                } else if tags.contains("config-example") {
                    let text = vec![];
                    let elem = if tags.contains("diff") {
                        ConfigSnippet::Diff(text)
                    } else {
                        ConfigSnippet::Full(text)
                    };
                    self.config_snippet = Some((elem, span))
                }
            }

            Event::Text(chunk) => {
                if let Some((ref mut text, _)) = self.mermaid {
                    text.push(chunk);
                } else if let Some((ConfigSnippet::Diff(ref mut text), _))
                | Some((ConfigSnippet::Full(ref mut text), _)) = self.config_snippet
                {
                    text.push(chunk);
                }
            }

            Event::End(TagEnd::CodeBlock) => {
                if let Some((elem, span)) = self.mermaid.take() {
                    return Some((Ok(Element::Mermaid(elem)), span));
                }
                if let Some((elem, span)) = self.config_snippet.take() {
                    return Some((Ok(Element::ConfigSnippet(elem)), span));
                }
            }

            _ => {}
        };

        None
    }
}
