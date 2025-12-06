use std::{borrow::Cow, collections::BTreeMap};

use miette::LabeledSpan;
use tap::TapFallible;
use url::Url;

use mdbookkit::diagnostics::{Diagnostics, Issue, IssueItem, ReportBuilder};

use crate::{
    Environment,
    link::{LinkStatus, RelativeLink},
    page::Pages,
};

impl Environment {
    pub fn report_issues<'a, F>(&'a self, content: &'a Pages<'a>, statuses: F) -> Reporter<'a>
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
                let text = content.get_text(base).unwrap();
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
        let link = self.shorten();
        let status = &self.link.status;
        let label = match status {
            LinkStatus::Ignored => None,
            LinkStatus::Published => Some(format!("file: {link}")),
            LinkStatus::Permalink => Some(format!("link: {link}")),
            LinkStatus::Rewritten => Some(format!("file: {link}\nlink: {}", self.link.link)),
            LinkStatus::PathNotCheckedIn => Some(format!("{status}: {link}")),
            LinkStatus::NoSuchPath => Some(format!("{status}: {link}")),
            LinkStatus::NoSuchFragment => {
                let (_, fragment) = self
                    .link
                    .link
                    .split_once('#')
                    .expect("should have a fragment");
                Some(format!("#{fragment} not found in {link}"))
            }
            LinkStatus::Error(..) => Some(format!("{status}")),
        };
        LabeledSpan::new_with_span(label, self.link.span.clone())
    }
}

impl LinkDiagnostic<'_> {
    fn shorten(&self) -> Cow<'_, str> {
        let Ok(link) = if self.link.link.starts_with('/') {
            self.root.join(&self.link.link[1..])
        } else {
            self.page.join(&self.link.link)
        }
        .tap_ok_mut(|u| u.set_fragment(None)) else {
            return Cow::Borrowed(&self.link.link);
        };
        let Some(rel) = self.root.make_relative(&link) else {
            return Cow::Borrowed(&self.link.link);
        };
        if rel.starts_with("../") {
            Cow::Owned(link.to_string())
        } else {
            Cow::Owned(rel)
        }
    }
}

impl Issue for LinkStatus {
    fn level(&self) -> log::Level {
        match self {
            Self::Ignored => log::Level::Debug,
            Self::Published => log::Level::Debug,
            Self::Rewritten => log::Level::Info,
            Self::Permalink => log::Level::Info,
            Self::PathNotCheckedIn => log::Level::Warn,
            Self::NoSuchPath => log::Level::Warn,
            Self::NoSuchFragment => log::Level::Warn,
            Self::Error(..) => log::Level::Warn,
        }
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
            Self::NoSuchFragment => 102,
            Self::NoSuchPath => 101,
            Self::PathNotCheckedIn => 100,
            Self::Permalink => 3,
            Self::Rewritten => 2,
            Self::Published => 1,
            Self::Ignored => 0,
        }
    }
}
