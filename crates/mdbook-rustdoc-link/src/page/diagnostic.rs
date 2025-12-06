use std::{fmt::Debug, hash::Hash};

use mdbookkit::diagnostics::{Diagnostics, ReportBuilder};

use super::{super::link::diagnostic::LinkDiagnostic, Pages};

impl<'a, K: Eq + Hash> Pages<'a, K> {
    pub fn diagnostics(&self) -> Vec<PageDiagnostics<'a, K>>
    where
        K: Clone,
    {
        self.pages
            .iter()
            .map(|(name, page)| {
                let issues = page.links.iter().map(|link| link.diagnostic()).collect();
                Diagnostics::new(page.source, name.clone(), issues)
            })
            .collect()
    }

    pub fn reporter(&self) -> PageReporter<'a, K>
    where
        K: Clone + Debug,
    {
        ReportBuilder::new(self.diagnostics(), |name| format!("{name:?}"))
    }
}

type PageDiagnostics<'a, K> = Diagnostics<'a, K, LinkDiagnostic>;

type PageReporter<'a, K> = ReportBuilder<'a, K, LinkDiagnostic, fn(&K) -> String>;
