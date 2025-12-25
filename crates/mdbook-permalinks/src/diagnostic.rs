use std::{borrow::Cow, collections::BTreeMap};

use miette::LabeledSpan;
use tap::{Pipe, TapOptional};
use tracing::Level;
use url::Url;

use mdbookkit::diagnostics::{Diagnostics, Issue, IssueItem, ReportBuilder};

use crate::{
    Environment,
    link::{LinkStatus, RelativeLink},
    page::Pages,
};

impl Environment {
    pub fn reporter<'a, F>(&'a self, content: &'a Pages<'a>, statuses: F) -> Reporter<'a>
    where
        F: Fn(&'a LinkStatus) -> bool,
    {
        // BTreeMap: sort output by paths
        let mut sorted: BTreeMap<&'_ Url, BTreeMap<LinkStatus, Vec<LinkDiagnostic<'_>>>> =
            Default::default();

        let root = &self.vcs.root;

        for (page, link) in content.links() {
            if !statuses(&link.status) {
                continue;
            }
            let diagnostic = LinkDiagnostic { link, page, root };
            sorted
                .entry(page)
                .or_default()
                .entry(link.status.clone())
                .or_default()
                .push(diagnostic);
        }

        let sorted = sorted
            .into_iter()
            .flat_map(|(base, issues)| {
                let text = content.get_text(base).expect("url should exist");
                issues
                    .into_values()
                    .map(|issues| Diagnostics::new(text, base, issues))
                    .collect::<Vec<_>>()
            })
            .collect();

        Reporter::new(sorted, Box::new(|url| self.rel_path(url)))
    }

    pub fn rel_path(&self, url: &Url) -> String {
        self.vcs
            .root
            .make_relative(url)
            .unwrap_or_else(|| url.as_str().into())
    }
}

type Reporter<'a> =
    ReportBuilder<'a, &'a Url, LinkDiagnostic<'a>, Box<dyn Fn(&'a Url) -> String + 'a>>;

pub struct LinkDiagnostic<'a> {
    link: &'a RelativeLink<'a>,
    page: &'a Url,
    root: &'a Url,
}

impl IssueItem for LinkDiagnostic<'_> {
    type Kind = LinkStatus;

    fn issue(&self) -> Self::Kind {
        self.link.status.clone()
    }

    fn label(&self) -> LabeledSpan {
        let link = &self.link.link;
        let path = self.shorten(&self.link.link);
        let status = &self.link.status;
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
        LabeledSpan::new_with_span(label, self.link.span.clone())
    }
}

impl LinkDiagnostic<'_> {
    fn shorten<'a>(&self, path: &'a str) -> Cow<'a, str> {
        let url = if let Some(path) = path.strip_prefix('/') {
            self.root.join(path)
        } else {
            self.page.join(path)
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

impl Issue for LinkStatus {
    fn level(&self) -> Level {
        match self {
            Self::Ignored => Level::TRACE,
            Self::Unchanged => Level::TRACE,
            Self::Rewritten => Level::DEBUG,
            Self::Permalink => Level::DEBUG,
            Self::Unreachable(..) => Level::WARN,
            Self::Error(..) => Level::ERROR,
        }
    }

    fn title(&self) -> impl std::fmt::Display {
        self
    }
}

impl PartialEq for LinkStatus {
    fn eq(&self, other: &Self) -> bool {
        self.order().eq(&other.order())
    }
}

impl Eq for LinkStatus {}

impl PartialOrd for LinkStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LinkStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order().cmp(&other.order())
    }
}

impl LinkStatus {
    fn order(&self) -> usize {
        match self {
            Self::Error(..) => 103,
            Self::Unreachable(..) => 101,
            Self::Permalink => 3,
            Self::Rewritten => 2,
            Self::Unchanged => 1,
            Self::Ignored => 0,
        }
    }
}
