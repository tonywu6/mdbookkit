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
            .flat_map(|(base, page)| page.rel_links.iter().map(move |link| (base, link)))
        {
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
        let label = match &self.link.status {
            LinkStatus::Ignored => None,
            LinkStatus::Published => None,
            LinkStatus::Rewritten => self.format_link().into_owned().pipe(Some),
            LinkStatus::Permalink => self.format_link().into_owned().pipe(Some),
            LinkStatus::External => {
                format!("file is outside source control: {}", self.format_link()).pipe(Some)
            }
            LinkStatus::NoSuchPath => {
                format!("file does not exist at path: {}", self.format_link()).pipe(Some)
            }
            LinkStatus::NoSuchFragment => {
                let (_, fragment) = self
                    .link
                    .link
                    .split_once('#')
                    .expect("should have a fragment");
                format!("#{fragment} not found in {}", self.format_link()).pipe(Some)
            }
            LinkStatus::Error(err) => format!("error converting to permalink:\n{err}").pipe(Some),
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
            Self::Rewritten => log::Level::Info,
            Self::Permalink => log::Level::Info,
            Self::External => log::Level::Warn,
            Self::NoSuchPath => log::Level::Warn,
            Self::NoSuchFragment => log::Level::Warn,
            Self::Error(..) => log::Level::Warn,
        }
    }
}

impl Display for LinkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            LinkStatus::Ignored => "url ignored",
            LinkStatus::Published => "file under src/",
            LinkStatus::Rewritten => "path converted to relative path",
            LinkStatus::Permalink => "path converted to permalink",
            LinkStatus::External => "file outside source control",
            LinkStatus::NoSuchPath => "file not found",
            LinkStatus::NoSuchFragment => "no such fragment",
            LinkStatus::Error(..) => "failed link conversion",
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
            Self::Error(..) => 7,
            Self::NoSuchFragment => 6,
            Self::NoSuchPath => 5,
            Self::External => 4,
            Self::Permalink => 3,
            Self::Rewritten => 2,
            Self::Published => 1,
            Self::Ignored => 0,
        }
    }
}
