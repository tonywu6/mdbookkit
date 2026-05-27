use std::{
    borrow::Cow,
    fmt::{Display, Write},
};

use tap::TapFallible;
use url::Url;

use mdbookkit::{
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, SourceCode,
        annotate_snippets::AnnotationKind,
    },
    error::ExpectFmt,
    url::UrlUtil,
};

use crate::{
    Environment,
    link::{LinkStatus, PathStatus, RelativeLink},
    page::Pages,
};

impl Environment<'_> {
    pub fn issues<'a, F>(&'a self, contents: &'a Pages<'a>, filter: F) -> Vec<IssueReporter<'a>>
    where
        F: Fn(&'a LinkStatus) -> bool,
    {
        let root = &self.vcs.root;
        (contents.pages())
            .map(|(base, page)| {
                let issues = (page.links())
                    .filter(|link| filter(&link.status))
                    .map(|link| LinkDiagnostic { root, base, link }.emit())
                    .collect();
                let source_code = page.source();
                let source_path = root.print_relative(base).to_string().into();
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
    link: &'a RelativeLink<'a>,
    base: &'a Url,
    root: &'a Url,
}

impl<'a> LinkDiagnostic<'a> {
    fn emit(&self) -> IssueReport<'a> {
        let RelativeLink {
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

                let notes = if candidates.len() > 1 {
                    let mut note = String::from("also tried the following paths");
                    for (link, status) in candidates.iter().skip(1) {
                        write!(note, "\n{:?}: path {status}", self.shorten_path(link)).expect_fmt();
                    }
                    vec![Note::note(note)]
                } else {
                    vec![]
                };

                IssueReport::level(IssueLevel::Warning)
                    .title(title)
                    .annotations(labels)
                    .notes(notes)
                    .build()
            }
        }
    }

    fn shorten_path<'p>(&self, path: &'p Url) -> Cow<'p, str> {
        if let path = self.root.print_relative(path).to_string()
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
            PathStatus::NotFound => "doesn't exist",
            PathStatus::NotADirectory => "exists but is not a directory",
            PathStatus::Unreachable => "is inaccessible",
            PathStatus::Ignored => "is ignored by git",
            PathStatus::NotInRepo => "is outside of this repo",
            PathStatus::NotInBook => "is not part of the book",
        };
        f.write_str(text)
    }
}
