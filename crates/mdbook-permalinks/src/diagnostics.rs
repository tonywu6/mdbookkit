use std::borrow::Cow;

use url::Url;

use mdbookkit::{
    diagnostics::{
        Highlight, IssueLevel, IssueReport, Note, Suggestion, annotate_snippets::AnnotationKind,
    },
    error::Show,
    url::UrlUtil,
};

use crate::{
    PREPROCESSOR_NAME,
    link::{BookPathError, Link, LinkHelp, LinkState, PathError},
};

pub struct LinkDiagnostic<'a, 'r> {
    pub link: &'a Link<'r>,
    pub base: &'a Url,
    pub root: &'a Url,
}

impl<'a: 'r, 'r> LinkDiagnostic<'a, 'r> {
    pub fn emit(&self) -> IssueReport<'r> {
        use {BookPathError::*, PathError::*};

        let span = self.link.span();
        let href = self.link.href();

        let error = match self.link.state() {
            Err(error) => error,
            Ok(state) => {
                return match state {
                    LinkState::Unsupported => IssueReport::level(IssueLevel::Note)
                        .title("the preprocessor does not support these links")
                        .annotations(vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Context)
                                .build(),
                        ])
                        .build(),

                    LinkState::BookLinkChecked => IssueReport::level(IssueLevel::Note)
                        .title("these links are valid")
                        .annotations(vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Context)
                                .build(),
                        ])
                        .build(),

                    LinkState::BookLinkUpdated => IssueReport::level(IssueLevel::Note)
                        .title("updated this link to be a relative path")
                        .annotations(vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(href)
                                .build(),
                        ])
                        .notes(vec![Note::note(
                            format! { "the resolved path is\n{:?}", self.shorten_href(href) },
                        )])
                        .build(),

                    LinkState::Permalink => IssueReport::level(IssueLevel::Note)
                        .title("updated this link to be a permalink")
                        .annotations(vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(href)
                                .build(),
                        ])
                        .notes(vec![Note::note(
                            format! { "the resolved path is\n{:?}", self.shorten_href(href) },
                        )])
                        .build(),
                };
            }
        };

        let title = match error.error {
            AmbiguousLinkToRoot => "ambiguous link to `/`".into(),
            _ => format!("broken link to {href:?}"),
        };

        let mut labels = vec![];
        let mut notes = vec![];
        let mut helps = vec![];

        if let NoSuchPage(ref err) = error.error {
            let path = self.root.as_base().show_path(&error.cause);

            macro_rules! assert_is_md {
                () => {{
                    let path = error.cause.path();
                    debug_assert! { path.ends_with(".md"), "{href:?} -> {:?}", error.cause.show() };
                }};
            }
            macro_rules! assert_is_http {
                () => {{
                    debug_assert! { href.starts_with("http:") || href.starts_with("https:"),
                    "{href:?} -> {:?}", error.cause.show() };
                }};
            }

            match err {
                NoResourceAtLocation(expected) => {
                    labels.extend([Highlight::span(span.clone())
                        .label("link doesn't match any file in the book")
                        .kind(AnnotationKind::Primary)
                        .build()]);

                    let expected = std::fmt::from_fn(|f| {
                        for c in expected {
                            let path = self.root.as_base().show_path(&c.cause);
                            write!(f, "\n{path:?}")?;
                        }
                        Ok(())
                    });

                    notes.extend([Note::help(format! {
                        "for this link to be accessible, expected any of the \
                        following files, but found none:{expected}"
                    })]);
                }

                DirectoryHasNoIndexFile => {
                    labels.extend([Highlight::span(span.clone())
                        .label("directory has no `index.md` file")
                        .kind(AnnotationKind::Primary)
                        .build()]);

                    notes.extend([
                        Note::help({
                            "without an `index.md` file at this location, the link will \
                            lead to a 404 error"
                        }),
                        Note::note(format!("the resolved path is {path:?}")),
                    ]);
                }

                MarkdownFileNotIncluded => {
                    assert_is_md!();

                    labels.extend([
                        Highlight::span(span.clone())
                            .label("file is not included in `SUMMARY.md`:")
                            .kind(AnnotationKind::Primary)
                            .build(),
                        Highlight::span(span.clone())
                            .label(format!("{path:?}"))
                            .kind(AnnotationKind::Context)
                            .build(),
                    ]);

                    notes.extend([Note::help({
                        "because this Markdown file is not referenced in `SUMMARY.md`, \
                        it will not be available in the output"
                    })]);
                }

                UnexpectedFileExtension => {
                    assert_is_md!();
                    assert_is_http!();

                    labels.extend([Highlight::span(span.clone())
                        .label("unexpected `.md` extension in link path")
                        .kind(AnnotationKind::Primary)
                        .build()]);

                    notes.extend([
                        Note::help({
                            "because the output path of this page won't contain \
                            the `.md` extension, this link won't work correctly"
                        }),
                        Note::note(format!("the resolved path is {path:?}")),
                    ]);
                }
            }
        } else if let AmbiguousLinkToRoot = error.error {
            labels.extend([Highlight::span(span.clone())
                .kind(AnnotationKind::Primary)
                .build()]);

            helps.extend([IssueReport::level(IssueLevel::Note)
                .title(concat! {
                    "with `", PREPROCESSOR_NAME!(), "`, this link could also \
                    mean a permalink to the root of the repository"
                })
                .build()]);
        } else {
            let shortened_path =
                if let Some(path) = self.root.as_base().make_relative_scoped(&error.cause) {
                    let path = path.show_path().to_string();
                    if href.strip_suffix(&path) == Some("/") {
                        None
                    } else {
                        Some(Cow::Owned(path))
                    }
                } else if let Ok(path) = error.cause.to_file_path() {
                    Some(Cow::Owned(path.display().to_string()))
                } else {
                    Some(Cow::Borrowed(error.cause.as_str()))
                };

            #[rustfmt::skip]
            let label: Cow<_> = match error.error {
                NotFound => {
                    "resolves to a path that doesn't exist".into()
                },
                NotADirectory => {
                    "resolves to a path that isn't a directory".into()
                },
                GitIgnored => {
                    "resolves to a path that is gitignored".into()
                },
                Inaccessible(e) => { format! {
                    "resolves to a path that cannot be accessed: I/O error: {e}"
                }.into() }
                NotInRepo => {
                    "resolves to a path that is outside of the repository".into()
                },
                InvalidEncoding => {
                    "link contains characters that are invalid on this system".into()
                }
                AmbiguousLinkToRoot => unreachable!(),
                NoSuchPage(..) => unreachable!(),
            };

            if let Some(path) = shortened_path {
                labels.extend([
                    Highlight::primary(span.clone(), format!("{label}:")),
                    Highlight::context(span.clone(), format!("{path:?}")),
                ])
            } else {
                labels.extend([Highlight::primary(span.clone(), label)]);
            }
        };

        match &error.help {
            Some(LinkHelp::FoundOther {
                from_page,
                from_repo,
            }) => {
                let from_repo = from_repo.clone().into_decoded();
                let from_page = from_page.clone().into_decoded();

                let note = format! {
                    "the following path is available: {:?}",
                    from_repo.show_path()
                };
                let note = IssueReport::level(IssueLevel::Note).title(note).build();

                let help1 = IssueReport::level(IssueLevel::Help)
                    .title("try using a relative path starting from the current page:")
                    .patches(vec![
                        Suggestion::span(span.clone())
                            .repl(from_page.consume_with(String::from))
                            .build(),
                    ])
                    .build();

                let help2 = IssueReport::level(IssueLevel::Help)
                    .title({
                        "... or use an absolute path starting \
                        from the root of your repository:"
                    })
                    .patches(vec![
                        Suggestion::span(span.clone())
                            .repl(from_repo.consume_with(String::from))
                            .build(),
                    ])
                    .notes(vec![Note::note(
                        concat! { "`", PREPROCESSOR_NAME!(), "`", " will convert",
                        " this path to a format accepted by mdBook" },
                    )])
                    .build();

                helps.extend([note, help1, help2]);
            }

            Some(LinkHelp::LinkToRoot {
                to_repo,
                to_book,
                to_book_relative,
            }) => {
                let help1 = IssueReport::level(IssueLevel::Help)
                    .title("to link to the root of the repo, try using a full URL:")
                    .patches(vec![Suggestion::span(span.clone()).repl(to_repo).build()])
                    .notes(vec![Note::note(
                        concat! { "`", PREPROCESSOR_NAME!(), "`", " will update this link",
                        " to point to the correct commit or tag" },
                    )])
                    .build();

                let help2 = if *to_book_relative {
                    IssueReport::level(IssueLevel::Help).title({
                        "to link to the homepage of the book, try using a \
                        relative path from the current page:"
                    })
                } else {
                    IssueReport::level(IssueLevel::Help).title({
                        "to link to the homepage of the book, try using an \
                        absolute path to the source directory:"
                    })
                }
                .patches(vec![Suggestion::span(span.clone()).repl(to_book).build()])
                .notes(vec![Note::note(
                    concat! { "`", PREPROCESSOR_NAME!(), "`", " will convert",
                    " this path to a format accepted by mdBook" },
                )])
                .build();

                helps.extend([help1, help2]);
            }

            Some(LinkHelp::GenericEdit { help, edited }) => {
                let help = IssueReport::level(IssueLevel::Help)
                    .title(*help)
                    .patches(vec![Suggestion::span(span.clone()).repl(edited).build()])
                    .build();

                helps.extend([help]);
            }

            None => {}
        }

        IssueReport::level(IssueLevel::Warning)
            .title(title)
            .annotations(labels)
            .notes(notes)
            .secondary(helps)
            .build()
    }

    fn shorten_href<'p>(&self, path: &'p str) -> Cow<'p, str> {
        let url = if let Some(path) = path.strip_prefix('/') {
            self.root.join(path)
        } else {
            self.base.join(path)
        };
        if let Ok(url) = url {
            self.root.as_base().show_path(&url).to_string().into()
        } else {
            Cow::Borrowed(path)
        }
    }
}
