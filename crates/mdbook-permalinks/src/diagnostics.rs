use std::{
    borrow::Cow,
    fmt::{Display, Write},
};

use tap::TapOptional;
use url::Url;

use mdbookkit::{
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, annotate_snippets::AnnotationKind,
    },
    error::ExpectFmt,
};

use crate::{
    Environment,
    link::{LinkStatus, PathStatus, RelativeLink},
    page::Pages,
};

impl Environment {
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
                let source_path = root
                    .make_relative(base)
                    .unwrap_or_else(|| base.as_str().into());
                IssueReporter {
                    issues,
                    source: (source_code, source_path).into(),
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
                    format! { "resolved path is {:?}", self.shorten(href) },
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
                    format! { "resolved path is {:?}",self.shorten(href) },
                )])
                .build(),

            LinkStatus::Unreachable(candidates) => {
                let title = format!("broken link to {href:?}");

                let labels = {
                    let (link, status) = &candidates[0];
                    let shortened = self.shorten_url(link);
                    if !shortened.starts_with('/') && href.strip_suffix(&*shortened) == Some("/") {
                        vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(format!("resolves to a path that is {status}"))
                                .build(),
                        ]
                    } else {
                        vec![
                            Highlight::span(span.clone())
                                .kind(AnnotationKind::Primary)
                                .label(format!("resolves to a path that is {status}:"))
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
                        write!(note, "\n{:?}: path is {status}", self.shorten_url(link))
                            .expect_fmt();
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

            LinkStatus::Error(error) => IssueReport::level(IssueLevel::Error)
                .title("error while resolving this link")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Primary)
                        .build(),
                ])
                .notes(vec![Note::note(error.to_string())])
                .build(),
        }
    }

    fn shorten<'p>(&self, path: &'p str) -> Cow<'p, str> {
        let url = if let Some(path) = path.strip_prefix('/') {
            self.root.join(path)
        } else {
            self.base.join(path)
        }
        .ok()
        .tap_some_mut(|url| {
            url.set_query(None);
            url.set_fragment(None);
        });
        if let Some(url) = url {
            self.shorten_url(&url).into_owned().into()
        } else {
            Cow::Borrowed(path)
        }
    }

    fn shorten_url<'p>(&self, url: &'p Url) -> Cow<'p, str> {
        if let Some(rel) = self.root.make_relative(url)
            && !rel.starts_with("../")
        {
            Cow::Owned(rel)
        } else if url.scheme() == "file" {
            match url.to_file_path() {
                Ok(path) => Cow::Owned(path.display().to_string()),
                Err(()) => Cow::Borrowed(url.path()),
            }
        } else {
            Cow::Borrowed(url.as_str())
        }
    }
}

impl Display for PathStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            PathStatus::Unreachable => "inaccessible",
            PathStatus::Ignored => "ignored by git",
            PathStatus::NotInRepo => "outside of this repo",
            PathStatus::NotInBook => "not in SUMMARY.md",
        };
        f.write_str(text)
    }
}
