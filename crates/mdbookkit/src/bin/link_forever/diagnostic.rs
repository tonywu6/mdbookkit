use std::{borrow::Cow, collections::BTreeMap};

use miette::LabeledSpan;
use tap::TapFallible;
use url::Url;

use crate::diagnostics::{Diagnostics, Issue, Problem, ReportBuilder};

use super::{Environment, LinkSpan, LinkStatus, LinkText, Pages, RelativeLink};

impl Environment {
    pub fn report<'a, F>(&'a self, content: &'a Pages<'a>, statuses: F) -> Reporter<'a>
    where
        F: Fn(&'a LinkStatus) -> bool,
    {
        // BTreeMap: sort output by paths
        let mut sorted: BTreeMap<&'_ Url, BTreeMap<LinkStatus, Vec<LinkDiagnostic<'_>>>> =
            Default::default();

        let root = &self.vcs_root;

        let iter = content.pages.iter().flat_map(|(base, page)| {
            page.links
                .iter()
                .flat_map(move |links| links.links().map(move |link| (base, link)))
        });

        for (base, link) in iter {
            if !statuses(&link.status) {
                continue;
            }
            let diagnostic = LinkDiagnostic { link, base, root };
            sorted
                .entry(base)
                .or_default()
                .entry(link.status.clone())
                .or_default()
                .push(diagnostic);
        }

        let sorted = sorted
            .into_iter()
            .flat_map(|(base, issues)| {
                let text = content.pages.get(base).unwrap().source;
                issues
                    .into_values()
                    .map(|issues| Diagnostics::new(text, base, issues))
                    .collect::<Vec<_>>()
            })
            .collect();

        Reporter::new(sorted, Box::new(|url| self.rel_path(url)))
    }

    pub fn rel_path(&self, url: &Url) -> String {
        self.vcs_root
            .make_relative(url)
            .unwrap_or_else(|| url.as_str().into())
    }
}

type Reporter<'a> =
    ReportBuilder<'a, &'a Url, LinkDiagnostic<'a>, Box<dyn Fn(&'a Url) -> String + 'a>>;

pub struct LinkDiagnostic<'a> {
    link: &'a RelativeLink<'a>,
    base: &'a Url,
    root: &'a Url,
}

impl Problem for LinkDiagnostic<'_> {
    type Kind = LinkStatus;

    fn issue(&self) -> Self::Kind {
        self.link.status.clone()
    }

    fn label(&self) -> LabeledSpan {
        let link = self.format_link();
        let status = &self.link.status;
        let label = match status {
            LinkStatus::Ignored => None,
            LinkStatus::PublishedPath => None,
            LinkStatus::RewrittenPath => Some(link.into_owned()),
            LinkStatus::PermalinkPath => Some(link.into_owned()),
            LinkStatus::PublishedHref => Some(link.into_owned()),
            LinkStatus::PermalinkHref => Some(link.into_owned()),
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
    fn format_link(&self) -> Cow<'_, str> {
        let Ok(link) = self
            .base
            .join(&self.link.link)
            .tap_ok_mut(|u| u.set_fragment(None))
        else {
            return Cow::Borrowed(&self.link.link);
        };
        let Some(rel) = self.root.make_relative(&link) else {
            return Cow::Borrowed(&self.link.link);
        };
        if rel.starts_with("../") {
            Cow::Owned(link.path().into())
        } else {
            Cow::Owned(rel)
        }
    }
}

impl Issue for LinkStatus {
    fn level(&self) -> log::Level {
        match self {
            Self::Ignored => log::Level::Debug,
            Self::PublishedPath => log::Level::Debug,
            Self::RewrittenPath => log::Level::Info,
            Self::PermalinkPath => log::Level::Info,
            Self::PublishedHref => log::Level::Info,
            Self::PermalinkHref => log::Level::Info,
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
            Self::Error(..) => 9,
            Self::NoSuchFragment => 8,
            Self::NoSuchPath => 7,
            Self::PathNotCheckedIn => 6,
            Self::PermalinkHref => 5,
            Self::PermalinkPath => 4,
            Self::PublishedHref => 3,
            Self::RewrittenPath => 2,
            Self::PublishedPath => 1,
            Self::Ignored => 0,
        }
    }
}

impl<'a> LinkSpan<'a> {
    fn links(&self) -> impl Iterator<Item = &'_ RelativeLink<'a>> {
        self.0.iter().filter_map(|item| match item {
            LinkText::Link(link) => Some(link),
            LinkText::Text(..) => None,
        })
    }
}
