//! Error reporting for preprocessors.

use std::{
    borrow::Borrow,
    fmt::{self, Debug, Display, Write},
};

use log::{Level, LevelFilter};
use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteError,
    MietteSpanContents, ReportHandler, Severity, SourceCode, SourceSpan, SpanContents,
};
use owo_colors::Style;
use tap::{Pipe, Tap};

/// Trait for Markdown diagnostics. This will eventually be printed to stderr.
///
/// Each [`IssueItem`] represents a specific message, such as a warning, associated with
/// an [`Issue`] (the type and severity of the issue) and a location in the Markdown
/// source, represented by [`LabeledSpan`].
pub trait IssueItem: Send + Sync {
    type Kind: Issue;
    fn issue(&self) -> Self::Kind;
    fn label(&self) -> LabeledSpan;
}

/// Trait for diagnostics classes. This is like a specific error code.
///
/// **For implementors:** The [`Display`] implementation, which is the title of each
/// diagnostic message, should use plurals whenever possible, because error reporters
/// may elect to group together multiple labels of the same [`Issue`]
pub trait Issue: Default + Debug + Display + Clone + Send + Sync {
    fn level(&self) -> Level;
}

/// A collection of [`Problem`]s associated with a Markdown file.
pub struct Diagnostics<'a, K, P> {
    text: &'a str,
    name: K,
    issues: Vec<P>,
}

impl<K, P> Diagnostics<'_, K, P>
where
    K: Title,
    P: IssueItem,
{
    /// Render a report of the diagnostics using [miette]'s graphical reporting
    pub fn to_report(&self, colored: bool) -> String {
        let handler = if colored {
            GraphicalTheme::unicode()
        } else {
            GraphicalTheme::unicode_nocolor()
        }
        .tap_mut(|t| t.characters.error = "error:".into())
        .tap_mut(|t| t.characters.warning = "warning:".into())
        .tap_mut(|t| t.characters.advice = "info:".into())
        .tap_mut(|t| t.styles.advice = Style::new().green().toggle(colored))
        .tap_mut(|t| t.styles.warning = Style::new().yellow().toggle(colored))
        .tap_mut(|t| t.styles.error = Style::new().red().toggle(colored))
        .tap_mut(|t| {
            // pre-emptively specify colors for all diagnostics, just for this collection
            // doing this because miette doesn't support associating colors with labels yet
            t.styles.highlights = if colored {
                self.issues
                    .iter()
                    .map(|item| level_style(item.issue().level()))
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

    /// Render the diagnostics as a list of log messages suitable for [`log`].
    pub fn to_logs(&self) -> String {
        let mut output = String::new();
        LoggingReportHandler
            .render_report(&mut output, self)
            .unwrap();
        output
    }
}

impl<'a, K, P> Diagnostics<'a, K, P>
where
    P: IssueItem,
{
    pub fn new(text: &'a str, name: K, issues: Vec<P>) -> Self {
        Self { text, name, issues }
    }

    pub fn filtered(self, level: LevelFilter) -> Option<Self> {
        let Self { text, name, issues } = self;
        let issues = issues
            .into_iter()
            .filter(|p| p.issue().level() <= level)
            .collect::<Vec<_>>();
        if issues.is_empty() {
            None
        } else {
            Some(Self { text, name, issues })
        }
    }

    pub fn name(&self) -> &K {
        &self.name
    }

    fn status(&self) -> P::Kind {
        self.issues
            .iter()
            .map(|p| p.issue())
            .min_by_key(|s| s.level())
            .unwrap_or_default()
    }
}

impl<K, P> Diagnostic for Diagnostics<'_, K, P>
where
    K: Title,
    P: IssueItem,
{
    fn severity(&self) -> Option<Severity> {
        match self.status().level() {
            Level::Error => Some(Severity::Error),
            Level::Warn => Some(Severity::Warning),
            _ => Some(Severity::Advice),
        }
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(self)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.issues.iter().map(|p| p.label())))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        // miette doesn't print the file name if there are no labels to report
        // so we print it here
        if self.issues.is_empty() {
            Some(Box::new(format!("in {}", self.name)))
        } else {
            None
        }
    }
}

impl<K, P> SourceCode for Diagnostics<'_, K, P>
where
    K: Title,
    P: Send + Sync,
{
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
        let inner = self
            .text
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

impl<K, P: IssueItem> Debug for Diagnostics<'_, K, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.status(), f)
    }
}

impl<K, P: IssueItem> Display for Diagnostics<'_, K, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.status(), f)
    }
}

impl<K, P: IssueItem> std::error::Error for Diagnostics<'_, K, P> {}

/// Builder for printing diagnostics over multiple files.
pub struct ReportBuilder<'a, K, P, F> {
    items: Vec<Diagnostics<'a, K, P>>,
    print_name: F,
    log_filter: LevelFilter,
    colored: bool,
    logging: bool,
}

impl<'a, K, P, F> ReportBuilder<'a, K, P, F> {
    pub fn new(items: Vec<Diagnostics<'a, K, P>>, print_name: F) -> Self {
        Self {
            items,
            print_name,
            log_filter: LevelFilter::Trace,
            colored: true,
            logging: true,
        }
    }

    /// Specify how file names should be printed.
    pub fn names<G>(self, print_name: G) -> ReportBuilder<'a, K, P, G>
    where
        G: for<'b> Fn(&'b K) -> String,
    {
        let Self {
            items,
            log_filter,
            colored,
            logging,
            ..
        } = self;
        ReportBuilder {
            items,
            print_name,
            log_filter,
            colored,
            logging,
        }
    }

    pub fn level(mut self, level: LevelFilter) -> Self {
        self.log_filter = level;
        self
    }

    pub fn colored(mut self, colored: bool) -> Self {
        self.colored = colored;
        self
    }

    pub fn named<Q>(mut self, mut f: impl FnMut(&K) -> bool) -> Self
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.items.retain(|d| f(&d.name));
        self
    }

    pub fn logging(mut self, logging: bool) -> Self {
        self.logging = logging;
        self
    }
}

impl<'a, K, P, F> ReportBuilder<'a, K, P, F>
where
    P: IssueItem,
{
    pub fn build(self) -> Reporter<'a, P>
    where
        F: for<'b> Fn(&'b K) -> String,
    {
        let Self {
            items,
            print_name,
            log_filter,
            colored,
            logging,
        } = self;

        let items = items
            .into_iter()
            .filter_map(|p| {
                if p.status().level() > log_filter {
                    return None;
                }

                let Diagnostics { text, name, issues } = p.filtered(log_filter)?;

                Some(Diagnostics {
                    text,
                    name: print_name(&name),
                    issues,
                })
            })
            .collect::<Vec<_>>();

        Reporter {
            items,
            colored,
            logging,
        }
    }
}

pub struct Reporter<'a, P> {
    items: Vec<Diagnostics<'a, String, P>>,
    colored: bool,
    logging: bool,
}

impl<P> Reporter<'_, P>
where
    P: IssueItem,
{
    pub fn to_status(&self) -> P::Kind {
        self.items
            .iter()
            .map(|p| p.status())
            .min_by_key(|s| s.level())
            .unwrap_or_default()
    }

    pub fn to_stderr(&self) -> &Self {
        if self.items.is_empty() {
            return self;
        }

        if self.logging {
            let status = self.to_status();
            let logs = self.to_logs();
            log::log!(status.level(), "{logs}");
        } else {
            let report = self.to_report();
            log::logger().flush();
            eprint!("\n\n{report}");
        };

        self
    }

    pub fn to_report(&self) -> String {
        self.items.iter().fold(String::new(), |mut out, diag| {
            writeln!(out, "{}", diag.to_report(self.colored)).unwrap();
            out
        })
    }

    pub fn to_logs(&self) -> String {
        self.items.iter().fold(String::new(), |mut out, diag| {
            writeln!(out, "{}", diag.to_logs()).unwrap();
            out
        })
    }
}

struct LoggingReportHandler;

impl LoggingReportHandler {
    fn render_report(&self, f: &mut impl fmt::Write, diagnostic: &dyn Diagnostic) -> fmt::Result {
        let level = match diagnostic.severity() {
            Some(Severity::Error) | None => "error",
            Some(Severity::Warning) => "warning",
            Some(Severity::Advice) => "info",
        };
        let code = if let Some(code) = diagnostic.code() {
            format!(" [{code}] ")
        } else {
            ": ".to_string()
        };
        write!(f, "{level}{code}{diagnostic}")?;

        if let Some(help) = diagnostic.help() {
            write!(f, "\nhelp: {help}")?;
        }

        if let Some(url) = diagnostic.url() {
            write!(f, "\nsee: {url}")?;
        }

        let (Some(labels), Some(source)) = (diagnostic.labels(), diagnostic.source_code()) else {
            return Ok(());
        };

        for label in labels {
            let source = source
                .read_span(label.inner(), 0, 0)
                .map_err(|_| fmt::Error)?;
            let path = source.name().unwrap_or("<anonymous>");
            let line = source.line() + 1;
            let column = source.column() + 1;
            if let Some(message) = label.label() {
                write!(f, "\n  {path}:{line}:{column}: {message}")?;
            } else {
                write!(f, "\n  {path}:{line}:{column}")?;
            }
        }

        Ok(())
    }
}

impl ReportHandler for LoggingReportHandler {
    fn debug(&self, error: &dyn Diagnostic, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            fmt::Debug::fmt(error, f)
        } else {
            self.render_report(f, error)
        }
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
    fn toggle(self, enabled: bool) -> Self;
}

impl StyleCompat for Style {
    fn toggle(self, enabled: bool) -> Self {
        if enabled { self } else { Style::new() }
    }
}

pub trait Title: Send + Sync + Display {}

impl<K: Send + Sync + Display> Title for K {}
