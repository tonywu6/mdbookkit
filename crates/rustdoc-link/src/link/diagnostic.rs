use log::Level;
use miette::LabeledSpan;

use super::{Link, LinkState};

pub struct LinkLabel {
    pub level: Level,
    pub label: LabeledSpan,
}

impl Link<'_> {
    pub fn to_label(&self) -> LinkLabel {
        let level = match self.state {
            LinkState::Unparsed => Level::Trace,
            LinkState::Parsed(_) => Level::Warn,
            LinkState::Resolved(_) => Level::Info,
        };
        let label = match &self.state {
            LinkState::Unparsed => Some(self.url.as_ref().into()),
            LinkState::Parsed(item) => Some(format!("failed to resolve links for {:?}", item.name)),
            LinkState::Resolved(links) => Some(format!("{}", links.url())),
        };
        let label = LabeledSpan::new_with_span(label, self.span.clone());
        LinkLabel { level, label }
    }
}
