use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fmt::{self, Display},
};

use miette::LabeledSpan;
use tap::{Pipe, TapFallible};
use url::Url;

use crate::diagnostics::{Diagnostics, Issue, Problem, ReportBuilder};

use super::{Environment, LinkStatus, Pages, RelativeLink};

impl Environment {
    pub fn report<'a>(&'a self, content: &'a Pages<'a>) -> Reporter<'a> {
        let mut sorted: HashMap<&'_ Url, BTreeMap<LinkStatus, Vec<LinkDiagnostic<'_>>>> =
            Default::default();

        let root = &self.vcs_root;

        for (base, link) in content
            .pages
            .iter()
            .flat_map(|(base, page)| page.rel_links.iter().map(|link| (base.as_ref(), link)))
        {
            let diagnostic = LinkDiagnostic { link, base, root };
            sorted
                .entry(base)
                .or_default()
                .entry(link.status)
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
    ReportBuilder<'a, &'a Url, LinkDiagnostic<'a>, LinkStatus, Box<dyn Fn(&'a Url) -> String + 'a>>;

pub struct LinkDiagnostic<'a> {
    link: &'a RelativeLink<'a>,
    base: &'a Url,
    root: &'a Url,
}

impl Problem for LinkDiagnostic<'_> {
    type Kind = LinkStatus;

    fn issue(&self) -> Self::Kind {
        self.link.status
    }

    fn label(&self) -> LabeledSpan {
        let label = match self.link.status {
            LinkStatus::Ignored => None,
            LinkStatus::Published => None,
            LinkStatus::Permalink => self.format_link().into_owned().pipe(Some),
            LinkStatus::External => {
                format!("file {} is outside source control", self.format_link()).pipe(Some)
            }
            LinkStatus::NoSuchPath => {
                format!("file {} does not exist", self.format_link()).pipe(Some)
            }
            LinkStatus::NoSuchFragment => {
                let (_, fragment) = self
                    .link
                    .link
                    .split_once('#')
                    .expect("should have a fragment");
                format!("#{fragment} not found in file {}", self.format_link()).pipe(Some)
            }
            LinkStatus::ParseError(err) => {
                format!("error converting to permalink:\n{err}").pipe(Some)
            }
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
            Self::Published => log::Level::Debug,
            Self::Permalink => log::Level::Info,
            Self::External => log::Level::Warn,
            Self::NoSuchPath => log::Level::Warn,
            Self::NoSuchFragment => log::Level::Warn,
            Self::ParseError(..) => log::Level::Warn,
        }
    }
}

impl Display for LinkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            LinkStatus::Ignored => "urls ignored",
            LinkStatus::Published => "paths under src/",
            LinkStatus::Permalink => "links converted to permalinks",
            LinkStatus::External => "paths outside source control",
            LinkStatus::NoSuchPath => "paths not found",
            LinkStatus::NoSuchFragment => "no such fragments",
            LinkStatus::ParseError(..) => "failed to convert links to permalinks",
        };
        f.write_str(message)
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
            Self::Ignored => 0,
            Self::Published => 1,
            Self::Permalink => 2,
            Self::External => 3,
            Self::NoSuchPath => 4,
            Self::NoSuchFragment => 5,
            Self::ParseError(..) => 6,
        }
    }
}
