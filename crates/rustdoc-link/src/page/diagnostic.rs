use std::fmt;

use console::colors_enabled_stderr;
use log::{Level, LevelFilter};
use lsp_types::Position;
use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteError,
    MietteSpanContents, Severity, SourceCode, SourceSpan, SpanContents,
};
use owo_colors::Style;
use tap::{Pipe, Tap};

use crate::{link::diagnostic::LinkLabel, logger::is_logging};

use super::{Page, Pages};

struct PageDiagnostic<'r, 'a, K> {
    page: &'r Page<'a>,
    name: K,
    items: Vec<LinkLabel>,
    lines: LineCounter,
    status: PageStatus,
}

impl<'r, 'a, K> PageDiagnostic<'r, 'a, K> {
    pub fn new(page: &'r Page<'a>, name: K, filter: LevelFilter) -> Self {
        let items = page
            .links
            .iter()
            .map(|link| link.to_label())
            .filter(|label| label.level <= filter)
            .collect::<Vec<_>>();

        let level = items.iter().map(|label| label.level).min();

        let status = match level {
            Some(Level::Warn) => PageStatus::Unresolved,
            Some(Level::Info) => PageStatus::Ok,
            _ => PageStatus::Debug,
        };

        let lines = LineCounter::new(page.source);

        Self {
            page,
            name,
            items,
            lines,
            status,
        }
    }
}

impl<K: PageName> Diagnostic for PageDiagnostic<'_, '_, K> {
    fn severity(&self) -> Option<Severity> {
        Some(self.status.severity())
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(self)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let iter = self.items.iter().map(|label| label.label.clone());
        Some(Box::new(iter))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        if self.items.is_empty() {
            Some(Box::new(format!("in {}", self.name.to_string())))
        } else {
            None
        }
    }
}

impl<K: PageName> SourceCode for PageDiagnostic<'_, '_, K> {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        let inner = self
            .page
            .source
            .read_span(span, context_lines_before, context_lines_after)?;
        let contents = MietteSpanContents::new_named(
            self.name.to_string(),
            inner.data(),
            *inner.span(),
            inner.line(),
            inner.column(),
            inner.line_count(),
        )
        .with_language("markdown");
        Ok(Box::new(contents))
    }
}

impl<K> fmt::Debug for PageDiagnostic<'_, '_, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.status, f)
    }
}

impl<K> fmt::Display for PageDiagnostic<'_, '_, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.status, f)
    }
}

impl<K> std::error::Error for PageDiagnostic<'_, '_, K> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PageStatus {
    Unresolved = 1,
    Ok,
    Debug,
}

impl PageStatus {
    fn severity(&self) -> Severity {
        match self {
            PageStatus::Unresolved => Severity::Warning,
            PageStatus::Ok => Severity::Advice,
            PageStatus::Debug => Severity::Advice,
        }
    }

    fn level(&self) -> Level {
        match self {
            PageStatus::Unresolved => Level::Warn,
            PageStatus::Ok => Level::Info,
            PageStatus::Debug => Level::Debug,
        }
    }
}

impl fmt::Display for PageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::Unresolved => "failed to resolve some links",
            Self::Ok => "successfully resolved all links",
            Self::Debug => "debug info",
        };
        fmt::Display::fmt(msg, f)
    }
}

impl<K: PageName> PageDiagnostic<'_, '_, K> {
    pub fn report(&self) -> String {
        let handler = if colors_enabled_stderr() {
            GraphicalTheme::unicode()
        } else {
            GraphicalTheme::unicode_nocolor()
        }
        .tap_mut(|t| t.characters.error = "error:".into())
        .tap_mut(|t| t.characters.warning = "warning:".into())
        .tap_mut(|t| t.characters.advice = "info:".into())
        .tap_mut(|t| t.styles.advice = Style::new().magenta().stderr())
        .tap_mut(|t| t.styles.warning = Style::new().yellow().stderr())
        .tap_mut(|t| t.styles.error = Style::new().red().stderr())
        .tap_mut(|t| {
            t.styles.highlights = if colors_enabled_stderr() {
                self.items
                    .iter()
                    .map(|item| level_style(item.level))
                    .collect()
            } else {
                vec![Style::new()]
            }
        })
        .pipe(GraphicalReportHandler::new_themed);

        let mut output = String::new();

        handler.render_report(&mut output, self).unwrap();

        output
    }

    pub fn logs(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.label.label().is_some())
            .map(|item| {
                let name = self.name.to_string();
                let msg = item.label.label().unwrap();
                let Position { line, character } = self.lines.lookup(item.label.offset());
                format!("{name}:{line}:{character}: {msg}")
            })
            .collect()
    }
}

pub struct Reporter<'r, 'a, K, F> {
    pages: &'r Pages<'a, K>,
    print_path: F,
    filter: LevelFilter,
}

impl<'a, K: fmt::Debug> Pages<'a, K> {
    pub fn reporter<'r>(&'r self) -> Reporter<'r, 'a, K, fn(&K) -> String> {
        Reporter {
            pages: self,
            print_path: |path| format!("{path:?}"),
            filter: LevelFilter::Warn,
        }
    }
}

impl<'r, 'a, K, F> Reporter<'r, 'a, K, F>
where
    K: 'a,
{
    pub fn level(mut self, level: LevelFilter) -> Self {
        self.filter = level;
        self
    }

    pub fn paths<G>(self, print_path: G) -> Reporter<'r, 'a, K, G>
    where
        G: Fn(&'a K) -> String,
    {
        let Self { pages, filter, .. } = self;
        Reporter {
            pages,
            print_path,
            filter,
        }
    }
}

impl<'r, 'a, K, F> Reporter<'r, 'a, K, F>
where
    F: Fn(&'a K) -> String,
    K: 'a,
    'r: 'a,
{
    pub fn report(self) -> Option<String> {
        self.diagnostics()
            .fold(String::new(), |mut out, diag| {
                use fmt::Write;
                writeln!(out, "{}", diag.report()).unwrap();
                out
            })
            .pipe(|report| {
                if report.is_empty() {
                    None
                } else {
                    Some(report)
                }
            })
    }

    pub fn logs(self) -> Vec<String> {
        self.diagnostics().flat_map(|diag| diag.logs()).collect()
    }

    pub fn stderr(self) {
        if is_logging() {
            let mut status = PageStatus::Debug;
            let logs = self
                .diagnostics()
                .inspect(|diag| {
                    if diag.status < status {
                        status = diag.status
                    }
                })
                .flat_map(|diag| diag.logs())
                .collect::<Vec<_>>()
                .join("\n  ");
            log::log!(status.level(), "{status}\n  {logs}");
        } else {
            let Some(report) = self.report() else { return };
            log::logger().flush();
            eprint!("\n\n{report}");
        }
    }

    fn diagnostics(self) -> impl Iterator<Item = PageDiagnostic<'r, 'a, String>> {
        self.pages
            .pages
            .iter()
            .map(move |(name, page)| {
                PageDiagnostic::new(page, (self.print_path)(name), self.filter)
            })
            .filter(|diag| !diag.items.is_empty())
    }
}

const fn level_style(level: Level) -> Style {
    match level {
        Level::Trace => Style::new().dimmed(),
        Level::Debug => Style::new().magenta(),
        Level::Info => Style::new().green(),
        Level::Warn => Style::new().yellow(),
        Level::Error => Style::new().red(),
    }
}

trait StyleCompat {
    fn stderr(self) -> Self;
}

impl StyleCompat for Style {
    fn stderr(self) -> Self {
        if colors_enabled_stderr() {
            self
        } else {
            Style::new()
        }
    }
}

struct LineCounter {
    newlines: Vec<usize>,
}

impl LineCounter {
    fn new(text: &str) -> Self {
        let newlines = text
            .char_indices()
            .filter_map(|(i, c)| if c == '\n' { Some(i) } else { None })
            .collect::<Vec<_>>();
        Self { newlines }
    }

    fn lookup(&self, pos: usize) -> Position {
        // Find the line by binary searching for the last newline before idx
        let (line, col) = match self.newlines.binary_search(&pos) {
            // Exactly on a newline, so start of next line
            Ok(line) => (line + 1, 0),
            // line is the insertion point, so it's the line after idx
            Err(idx) => {
                if idx == 0 {
                    // First line, column is just the index
                    (0, pos)
                } else {
                    // Subsequent lines, column is the offset from the previous newline
                    let prev = self.newlines[idx - 1];
                    (idx, pos - prev - 1) // -1 to skip the newline character
                }
            }
        };
        Position::new(line as _, col as _)
    }
}

pub trait PageName: Send + Sync + ToString {}

impl<K: Send + Sync + ToString> PageName for K {}
