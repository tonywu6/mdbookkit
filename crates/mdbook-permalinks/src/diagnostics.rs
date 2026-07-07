use std::borrow::Cow;

use tap::Pipe;
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
    link::{BookPathError, Link, LinkHelp, LinkSpan, LinkState, PathError},
};

pub fn link_issue<'a, 'r>(
    root: &'a Url,
    page: &'a Url,
    link: &'a Link<'r>,
) -> Option<IssueReport<'r>> {
    use {BookPathError::*, PathError::*};

    let span = match link.span() {
        LinkSpan::Exact(span) => span,
        LinkSpan::Fuzzy(span) => span,
    };
    let href = link.href();

    let error = match link.state() {
        Err(error) => error,
        Ok(state) => {
            let shorten_href = |path: &'a str| {
                let url = if let Some(path) = path.strip_prefix('/') {
                    root.join(path)
                } else {
                    page.join(path)
                };
                if let Ok(url) = url {
                    root.as_base().show_path(&url).to_string().into()
                } else {
                    Cow::Borrowed(path)
                }
            };

            return IssueReport::if_enabled(IssueLevel::Note).map(|issue| match state {
                LinkState::Unsupported => issue
                    .title("the preprocessor does not support these links")
                    .annotations(vec![
                        Highlight::span(span.clone())
                            .kind(AnnotationKind::Context)
                            .build(),
                    ])
                    .build(),

                LinkState::BookLinkChecked => issue
                    .title("these links are valid")
                    .annotations(vec![
                        Highlight::span(span.clone())
                            .kind(AnnotationKind::Context)
                            .build(),
                    ])
                    .build(),

                LinkState::BookLinkUpdated => issue
                    .title("updated this link to be a relative path")
                    .annotations(vec![
                        Highlight::span(span.clone())
                            .kind(AnnotationKind::Primary)
                            .label(href.to_owned())
                            .build(),
                    ])
                    .notes(vec![Note::note(
                        format! { "the resolved path is\n{:?}", shorten_href(href) },
                    )])
                    .build(),

                LinkState::Permalink => issue
                    .title("updated this link to be a permalink")
                    .annotations(vec![
                        Highlight::span(span.clone())
                            .kind(AnnotationKind::Primary)
                            .label(href.to_owned())
                            .build(),
                    ])
                    .notes(vec![Note::note(
                        format! { "the resolved path is\n{:?}", shorten_href(href) },
                    )])
                    .build(),
            });
        }
    };

    let title = match error.error {
        AmbiguousLinkToRoot => "ambiguous link to `/`".into(),
        _ => format!("broken link to {href:?}"),
    };

    let mut labels = vec![];
    let mut notes = vec![];
    let mut helps: Vec<IssueReport<'r>> = vec![];

    if let NoSuchPage(ref err) = error.error {
        let path = root.as_base().show_path(&error.cause);

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
                        let path = root.as_base().show_path(&c.cause);
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
        let shortened_path = if let Some(path) = root.as_base().make_relative_scoped(&error.cause) {
            let path = path.show_path().to_string();
            // don't repeat the path in the labels if it looks
            // the same as the highlighted Markdown (which would be
            // if the Markdown link already specified a full path)
            if href.len() == span.len() && href.strip_suffix(&path) == Some("/") {
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

    let send_patch = |message: &'r str, patch: String| match link.span() {
        LinkSpan::Exact(span) => IssueReport::level(IssueLevel::Help)
            .title(format!("{message}:"))
            .patches(vec![Suggestion::span(span.clone()).repl(patch).build()]),
        LinkSpan::Fuzzy(_) => IssueReport::level(IssueLevel::Help)
            .title(format!("{message}:\n  {patch:?}"))
            .patches(vec![]),
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

            let help1 = send_patch(
                "try using a relative path starting from the current page",
                from_page.consume_with(String::from),
            )
            .build();

            let help2 = send_patch(
                "... or use an absolute path starting from the root of your repository",
                from_repo.consume_with(String::from),
            )
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
            let help1 = send_patch(
                "to link to the root of the repo, try using a full URL",
                to_repo.clone(),
            )
            .notes(vec![Note::note(
                concat! { "`", PREPROCESSOR_NAME!(), "`", " will update this link",
                " to point to the correct commit or tag" },
            )])
            .build();

            let help2 = send_patch(
                if *to_book_relative {
                    "to link to the homepage of the book, try using a \
                     relative path from the current page"
                } else {
                    "to link to the homepage of the book, try using an \
                    absolute path to the source directory"
                },
                to_book.clone(),
            )
            .notes(vec![Note::note(
                concat! { "`", PREPROCESSOR_NAME!(), "`", " will convert",
                " this path to a format accepted by mdBook" },
            )])
            .build();

            helps.extend([help1, help2]);
        }

        Some(LinkHelp::GenericEdit { help, edited }) => {
            let help = send_patch(help, edited.clone()).build();

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
        .pipe(Some)
}
