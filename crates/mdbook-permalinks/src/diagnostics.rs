use std::{
    borrow::Cow,
    fmt::{Display, Write},
};

use tap::TapFallible;
use url::Url;

use mdbookkit::{
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, SourceCode, Suggestion,
        annotate_snippets::AnnotationKind,
    },
    error::{ExpectFmt, Show},
    url::UrlUtil,
};

use crate::{
    Environment, PREPROCESSOR_NAME,
    link::{Link, LinkStatus, PathFixes, PathStatus},
    page::Pages,
};

impl Environment<'_> {
    pub fn issues<'a>(&'a self, contents: &'a Pages<'a>) -> Vec<IssueReporter<'a>> {
        let root = &self.vcs.root;
        (contents.pages())
            .map(|(base, page)| {
                let issues = (page.links())
                    .map(|link| LinkDiagnostic { root, base, link }.emit())
                    .collect();
                let source_code = page.source();
                let source_path = root.as_base().show_relative(base).to_string().into();
                IssueReporter {
                    issues,
                    source: SourceCode {
                        source_code,
                        source_path,
                    },
                }
            })
            .collect()
    }
}

struct LinkDiagnostic<'a> {
    link: &'a Link<'a>,
    base: &'a Url,
    root: &'a Url,
}

impl<'a> LinkDiagnostic<'a> {
    fn emit(&self) -> IssueReport<'a> {
        let Link {
            status, span, href, ..
        } = self.link;

        let span = span.any();
        let href = &**href;

        match status {
            LinkStatus::Ignored => IssueReport::level(IssueLevel::Note)
                .title("these links are not processed")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Context)
                        .build(),
                ])
                .build(),

            LinkStatus::Unchanged => IssueReport::level(IssueLevel::Note)
                .title("these links point to book pages or static files")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Context)
                        .build(),
                ])
                .build(),

            LinkStatus::Rewritten => IssueReport::level(IssueLevel::Note)
                .title("rewrote this link as a relative path")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Primary)
                        .label(href)
                        .build(),
                ])
                .notes(vec![Note::note(
                    format! { "resolved path is {:?}", self.shorten_href(href) },
                )])
                .build(),

            LinkStatus::Permalink => IssueReport::level(IssueLevel::Note)
                .title("rewrote this link as a permalink")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Primary)
                        .label(href)
                        .build(),
                ])
                .notes(vec![Note::note(
                    format! { "resolved path is {:?}", self.shorten_href(href) },
                )])
                .build(),

            LinkStatus::Unreachable(candidates) => {
                let title = format!("broken link to {href:?}");

                let labels = {
                    let (link, status) = &candidates[0];
                    let shortened = self.shorten_path(link);
                    if !shortened.starts_with('/') && href.strip_suffix(&*shortened) == Some("/") {
                        vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(format!("resolves to a path that {status}"))
                                .build(),
                        ]
                    } else {
                        vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(format!("resolves to a path that {status}:"))
                                .build(),
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Context)
                                .label(format!("{shortened:?}"))
                                .build(),
                        ]
                    }
                };

                let mut notes = if candidates.len() > 1 {
                    let mut note = String::from("also tried the following paths");
                    for (link, status) in candidates.iter().skip(1) {
                        write!(note, "\n{:?}: path {status}", self.shorten_path(link)).expect_fmt();
                    }
                    vec![Note::note(note)]
                } else {
                    vec![]
                };

                let mut helps = vec![];

                if let PathStatus::NotFound {
                    fix:
                        Some(PathFixes {
                            ref relative,
                            ref absolute,
                        }),
                } = candidates[0].1
                {
                    let absolute = (absolute.clone())
                        .into_decoded()
                        .consume_with(std::convert::identity);

                    let relative = (relative.clone())
                        .into_decoded()
                        .consume_with(std::convert::identity);

                    let note = format! {
                        "the following path is available:\n{:?}\n",
                        absolute.show()
                    };

                    let help1 = IssueReport::level(IssueLevel::Help)
                        .title("try using a relative path starting from the current page:")
                        .patches(vec![Suggestion::span(span.clone()).repl(relative).build()])
                        .build();

                    let help2 = IssueReport::level(IssueLevel::Help)
                        .title({
                            "... or use an absolute path starting \
                            from the root of your repository:"
                        })
                        .patches(vec![Suggestion::span(span.clone()).repl(absolute).build()])
                        .notes(vec![Note::help(
                            concat! { "`", PREPROCESSOR_NAME!(), "`", " will convert ",
                            "this path to a format accepted by mdBook" },
                        )])
                        .build();

                    notes.push(Note::note(note));
                    helps.extend([help1, help2]);
                }

                IssueReport::level(IssueLevel::Warning)
                    .title(title)
                    .annotations(labels)
                    .notes(notes)
                    .secondary(helps)
                    .build()
            }
        }
    }

    fn shorten_path<'p>(&self, path: &'p Url) -> Cow<'p, str> {
        if let path = self.root.as_base().show_relative(path).to_string()
            && !path.starts_with("../")
        {
            path.into()
        } else if path.scheme() == "file" {
            match path.to_file_path() {
                Ok(path) => Cow::Owned(path.display().to_string()),
                Err(()) => Cow::Borrowed(path.path()),
            }
        } else {
            Cow::Borrowed(path.as_str())
        }
    }

    fn shorten_href<'p>(&self, path: &'p str) -> Cow<'p, str> {
        let url = if let Some(path) = path.strip_prefix('/') {
            self.root.join(path)
        } else {
            self.base.join(path)
        }
        .tap_ok_mut(|url| {
            url.set_query(None);
            url.set_fragment(None);
        });
        if let Ok(url) = url {
            self.shorten_path(&url).into_owned().into()
        } else {
            Cow::Borrowed(path)
        }
    }
}

impl Display for PathStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            PathStatus::NotFound { .. } => "doesn't exist",
            PathStatus::NotADirectory => "exists but is not a directory",
            PathStatus::Unreachable => "is inaccessible",
            PathStatus::GitIgnored => "is ignored by git",
            PathStatus::NotInRepo => "is outside of this repo",
            PathStatus::NotInBook => "is not part of the book",
            PathStatus::InvalidBytes => "is invalid on this system",
        };
        f.write_str(text)
    }
}
