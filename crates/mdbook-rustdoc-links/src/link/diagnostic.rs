use std::fmt;

use miette::LabeledSpan;
use tracing::Level;

use mdbookkit::diagnostics::{Issue, IssueItem};

use super::{Link, LinkState};

pub struct LinkDiagnostic {
    pub status: LinkStatus,
    pub label: LabeledSpan,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkStatus {
    Unresolved = 1,
    Ok,
    #[default]
    Debug,
}

impl IssueItem for LinkDiagnostic {
    type Kind = LinkStatus;

    fn issue(&self) -> Self::Kind {
        self.status
    }

    fn label(&self) -> LabeledSpan {
        self.label.clone()
    }
}

impl Issue for LinkStatus {
    fn level(&self) -> Level {
        match self {
            Self::Unresolved => Level::WARN,
            Self::Debug => Level::TRACE,
            Self::Ok => Level::DEBUG,
        }
    }

    fn title(&self) -> impl fmt::Display {
        match self {
            Self::Unresolved => "item could not be resolved",
            Self::Ok => "link resolved",
            Self::Debug => "debug info",
        }
    }
}

impl Link<'_> {
    pub fn diagnostic(&self) -> LinkDiagnostic {
        let status = match self.state {
            LinkState::Unparsed => LinkStatus::Debug,
            LinkState::Pending(_) => LinkStatus::Unresolved,
            LinkState::Resolved(_) => LinkStatus::Ok,
        };
        let label = match &self.state {
            LinkState::Unparsed => Some(self.url.as_ref().into()),
            LinkState::Pending(item) => Some(format!("could not obtain a link to {:?}", item.name)),
            LinkState::Resolved(links) => Some(format!("{}", links.url())),
        };
        let label = LabeledSpan::new_with_span(label, self.span.clone());
        LinkDiagnostic { status, label }
    }
}
