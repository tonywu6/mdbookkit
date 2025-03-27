use std::fmt;

use log::Level;
use miette::LabeledSpan;

use crate::diagnostics::{Issue, Problem};

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

impl Problem for LinkDiagnostic {
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
            Self::Unresolved => Level::Warn,
            Self::Debug => Level::Trace,
            Self::Ok => Level::Info,
        }
    }
}

impl fmt::Display for LinkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::Unresolved => "failed to resolve some links",
            Self::Ok => "successfully resolved all links",
            Self::Debug => "debug info",
        };
        fmt::Display::fmt(msg, f)
    }
}

impl Link<'_> {
    pub fn diagnostic(&self) -> LinkDiagnostic {
        let status = match self.state {
            LinkState::Unparsed => LinkStatus::Debug,
            LinkState::Parsed(_) => LinkStatus::Unresolved,
            LinkState::Resolved(_) => LinkStatus::Ok,
        };
        let label = match &self.state {
            LinkState::Unparsed => Some(self.url.as_ref().into()),
            LinkState::Parsed(item) => Some(format!("failed to resolve links for {:?}", item.name)),
            LinkState::Resolved(links) => Some(format!("{}", links.url())),
        };
        let label = LabeledSpan::new_with_span(label, self.span.clone());
        LinkDiagnostic { status, label }
    }
}
