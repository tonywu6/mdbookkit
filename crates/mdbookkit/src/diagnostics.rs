use std::fmt::{self, Debug, Display, Write};

use log::{Level, LevelFilter};
use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteError,
    MietteSpanContents, ReportHandler, Severity, SourceCode, SourceSpan, SpanContents,
};
use owo_colors::Style;
use tap::{Pipe, Tap};

pub trait Problem: Send + Sync {
    type Kind: Issue;
    fn issue(&self) -> Self::Kind;
    fn label(&self) -> LabeledSpan;
}

pub trait Issue: Default + Debug + Display + Clone + Send + Sync {
    fn level(&self) -> Level;
}

pub struct Diagnostics<'a, K, P, S> {
    text: &'a str,
    name: K,
    issues: Vec<P>,
    status: S,
}

impl<K, P> Diagnostics<'_, K, P, P::Kind>
where
    K: Title,
    P: Problem,
{
    pub fn to_report(&self, colored: bool) -> String {
        let handler = if colored {
            GraphicalTheme::unicode()
        } else {
            GraphicalTheme::unicode_nocolor()
        }
        .tap_mut(|t| t.characters.error = "error:".into())
        .tap_mut(|t| t.characters.warning = "warning:".into())
        .tap_mut(|t| t.characters.advice = "info:".into())
        .tap_mut(|t| t.styles.advice = Style::new().magenta().toggle(colored))
        .tap_mut(|t| t.styles.warning = Style::new().yellow().toggle(colored))
        .tap_mut(|t| t.styles.error = Style::new().red().toggle(colored))
        .tap_mut(|t| {
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

    pub fn to_logs(&self) -> String {
        let mut output = String::new();
        LoggingReportHandler
            .render_report(&mut output, self)
            .unwrap();
        output
    }
}

impl<'a, K, P> Diagnostics<'a, K, P, P::Kind>
where
    P: Problem,
{
    pub fn new(text: &'a str, name: K, issues: Vec<P>) -> Self {
        let status = issues
            .iter()
            .map(|p| p.issue())
            .min_by_key(|s| s.level())
            .unwrap_or_default();
        Self {
            text,
            name,
            issues,
            status,
        }
    }

    pub fn filtered(self, level: LevelFilter) -> Option<Self> {
        let Self {
            text,
            name,
            issues,
            status,
        } = self;
        let issues = issues
            .into_iter()
            .filter(|p| p.issue().level() <= level)
            .collect::<Vec<_>>();
        if issues.is_empty() {
            None
        } else {
            Some(Self {
                text,
                name,
                issues,
                status,
            })
        }
    }
}

impl<K, P> Diagnostic for Diagnostics<'_, K, P, P::Kind>
where
    K: Title,
    P: Problem,
{
    fn severity(&self) -> Option<Severity> {
        match self.status.level() {
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
        if self.issues.is_empty() {
            Some(Box::new(format!("in {}", self.name)))
        } else {
            None
        }
    }
}

impl<K, P, S> SourceCode for Diagnostics<'_, K, P, S>
where
    K: Title,
    P: Send + Sync,
    S: Send + Sync,
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

impl<K, P: Problem> Debug for Diagnostics<'_, K, P, P::Kind> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.status, f)
    }
}

impl<K, P: Problem> Display for Diagnostics<'_, K, P, P::Kind> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.status, f)
    }
}

impl<K, P: Problem> std::error::Error for Diagnostics<'_, K, P, P::Kind> {}

pub struct ReportBuilder<'a, K, P, S, F> {
    items: Vec<Diagnostics<'a, K, P, S>>,
    print_name: F,
    log_filter: LevelFilter,
    colored: bool,
    logging: bool,
}

impl<'a, K, P, S, F> ReportBuilder<'a, K, P, S, F> {
    pub fn new(items: Vec<Diagnostics<'a, K, P, S>>, print_name: F) -> Self {
        Self {
            items,
            print_name,
            log_filter: LevelFilter::Trace,
            colored: true,
            logging: true,
        }
    }

    pub fn names<G>(self, print_name: G) -> ReportBuilder<'a, K, P, S, G>
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

    pub fn logging(mut self, logging: bool) -> Self {
        self.logging = logging;
        self
    }
}

impl<'a, K, P, F> ReportBuilder<'a, K, P, P::Kind, F>
where
    P: Problem,
{
    pub fn build(self) -> Reporter<'a, P, P::Kind>
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
                if p.status.level() > log_filter {
                    return None;
                }

                let Diagnostics {
                    text,
                    name,
                    issues,
                    status,
                } = p.filtered(log_filter)?;

                Some(Diagnostics {
                    text,
                    name: print_name(&name),
                    issues,
                    status,
                })
            })
            .collect::<Vec<_>>();

        let status = items
            .iter()
            .map(|p| p.status.clone())
            .min_by_key(|s| s.level())
            .unwrap_or_default();

        Reporter {
            items,
            status,
            colored,
            logging,
        }
    }
}

pub struct Reporter<'a, P, S> {
    items: Vec<Diagnostics<'a, String, P, S>>,
    status: S,
    colored: bool,
    logging: bool,
}

impl<P> Reporter<'_, P, P::Kind>
where
    P: Problem,
{
    pub fn to_status(&self) -> P::Kind {
        self.status.clone()
    }

    pub fn to_stderr(&self) -> &Self {
        if self.items.is_empty() {
            return self;
        }

        if self.logging {
            let status = self.status.clone();
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
    fn render_report(&self, f: &mut impl fmt::Write, diagnostic: &(dyn Diagnostic)) -> fmt::Result {
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
        if enabled {
            self
        } else {
            Style::new()
        }
    }
}

pub trait Title: Send + Sync + Display {}

impl<K: Send + Sync + Display> Title for K {}
