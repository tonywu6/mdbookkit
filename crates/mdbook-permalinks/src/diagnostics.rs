use std::borrow::Cow;

use tap::{Pipe, TapOptional};
use url::Url;

use mdbookkit::diagnostics::{
    Highlight, IssueLevel, IssueReport, IssueReporter, annotate_snippets::AnnotationKind,
};

use crate::{
    Environment,
    link::{LinkStatus, RelativeLink},
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
        IssueReport::level((&self.link.status).into())
            .title(self.link.status.to_string())
            .annotations(vec![self.label()])
            .build()
    }

    fn label(&self) -> Highlight<'a> {
        let RelativeLink {
            status, span, link, ..
        } = self.link;
        let path = self.shorten(link);
        let label = match status {
            LinkStatus::Ignored => None,
            LinkStatus::Unchanged => Some(path.into()),
            LinkStatus::Permalink => Some(path.into()),
            LinkStatus::Rewritten => Some(format!("path: {path}\nlink: {:?}", &**link)),
            LinkStatus::Unreachable(errors) => errors
                .iter()
                .filter_map(|(url, err)| self.shorten_url(url).map(|url| (url, err)))
                .fold(String::new(), |mut msg, (url, err)| {
                    msg.push_str(&url);
                    msg.push(' ');
                    msg.push_str(&err.to_string());
                    msg.push('\n');
                    msg
                })
                .trim()
                .to_owned()
                .pipe(Some),
            LinkStatus::Error(..) => Some(status.to_string()),
        };
        Highlight::span(span.clone())
            .kind(AnnotationKind::Primary)
            .maybe_label(label)
            .build()
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
            if let Some(shortened) = self.shorten_url(&url) {
                Cow::Owned(shortened)
            } else {
                Cow::Borrowed(path)
            }
        } else {
            Cow::Borrowed(path)
        }
    }

    fn shorten_url(&self, url: &Url) -> Option<String> {
        if let Some(rel) = self.root.make_relative(url)
            && !rel.starts_with("../")
        {
            Some(rel)
        } else if url.scheme() == "file" {
            match url.to_file_path() {
                Ok(path) => Some(path.display().to_string()),
                Err(()) => Some(url.path().to_owned()),
            }
        } else {
            None
        }
    }
}

impl From<&LinkStatus> for IssueLevel {
    fn from(value: &LinkStatus) -> Self {
        match value {
            LinkStatus::Ignored => IssueLevel::Note,
            LinkStatus::Unchanged => IssueLevel::Note,
            LinkStatus::Rewritten => IssueLevel::Note,
            LinkStatus::Permalink => IssueLevel::Note,
            LinkStatus::Unreachable(..) => IssueLevel::Warning,
            LinkStatus::Error(..) => IssueLevel::Error,
        }
    }
}
