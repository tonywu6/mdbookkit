//! Error reporting for preprocessors.

use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fmt::{self, Write as _},
    io::Write as _,
};

use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteError,
    MietteSpanContents, Severity, SourceCode, SourceSpan, SpanContents,
};
use owo_colors::Style;
use tap::{Pipe, Tap, TapFallible};
use tracing::{Level, debug, error, info, level_filters::LevelFilter, trace, warn};

use crate::{
    emit_debug,
    env::{is_colored, is_logging},
    error::{ExpectFmt, put_severity},
    logging::stderr,
};

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

/// Trait for diagnostics classes, like an error code.
pub trait Issue: fmt::Debug + Default + Clone + Send + Sync {
    fn title(&self) -> impl fmt::Display;
    fn level(&self) -> Level;
}

/// A collection of [`IssueItem`]s associated with a Markdown file.
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
    pub fn to_report(&self) -> String {
        let handler = if is_colored() {
            GraphicalTheme::unicode()
        } else {
            GraphicalTheme::unicode_nocolor()
        }
        .tap_mut(|t| t.characters.error = "error:".into())
        .tap_mut(|t| t.characters.warning = "warning:".into())
        .tap_mut(|t| t.characters.advice = "info:".into())
        .tap_mut(|t| t.styles.advice = Style::new().green().stderr())
        .tap_mut(|t| t.styles.warning = Style::new().yellow().stderr())
        .tap_mut(|t| t.styles.error = Style::new().red().stderr())
        .tap_mut(|t| {
            // pre-emptively specify colors for all diagnostics, just for this collection
            // doing this because miette doesn't support associating colors with labels yet
            t.styles.highlights = if is_colored() {
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
        handler.render_report(&mut output, self).expect_fmt();
        output
    }

    pub fn to_traces(&self) {
        for item in self.issues.iter() {
            let issue = item.issue();
            let label = item.label();
            let source = self
                .read_span(label.inner(), 0, 0)
                .expect("self.read_span infallible");
            let path = source.name().unwrap_or("<anonymous>");
            let line = source.line() + 1;
            let column = source.column() + 1;
            let title = issue.title();
            let level = issue.level();
            let label = label.label().unwrap_or_default();
            let message = format_args!("{path}:{line}:{column}: {title}");
            let message = if label.is_empty() {
                message
            } else {
                format_args!("{message}: {label}")
            };
            if level >= Level::TRACE {
                trace!("{message}")
            } else if level >= Level::DEBUG {
                debug!("{message}")
            } else if level >= Level::INFO {
                info!("{message}")
            } else if level >= Level::WARN {
                warn!("{message}")
            } else {
                error!("{message}")
            }
        }
    }
}

impl<'a, K, P> Diagnostics<'a, K, P>
where
    P: IssueItem,
{
    pub fn new(text: &'a str, name: K, issues: Vec<P>) -> Self {
        Self { text, name, issues }
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
            Level::ERROR => Some(Severity::Error),
            Level::WARN => Some(Severity::Warning),
            _ => Some(Severity::Advice),
        }
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(self)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.issues.iter().map(|p| p.label())))
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
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

impl<K, P: IssueItem> fmt::Debug for Diagnostics<'_, K, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.status(), f)
    }
}

impl<K, P: IssueItem> fmt::Display for Diagnostics<'_, K, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.status().title(), f)
    }
}

impl<K, P: IssueItem> std::error::Error for Diagnostics<'_, K, P> {}

/// Builder for printing diagnostics over multiple files.
pub struct ReportBuilder<'a, K, P, F> {
    items: Vec<Diagnostics<'a, K, P>>,
    name_display: F,
    level_filter: LevelFilter,
}

impl<'a, K, P, F> ReportBuilder<'a, K, P, F> {
    pub fn new(items: Vec<Diagnostics<'a, K, P>>, name_display: F) -> Self {
        Self {
            items,
            name_display,
            level_filter: max_level(),
        }
    }

    /// Specify how file names should be printed.
    pub fn name_display<G>(self, name_display: G) -> ReportBuilder<'a, K, P, G>
    where
        G: for<'b> Fn(&'b K) -> String,
    {
        let Self {
            items,
            level_filter,
            ..
        } = self;
        ReportBuilder {
            items,
            name_display,
            level_filter,
        }
    }

    pub fn level_filter(mut self, level: LevelFilter) -> Self {
        self.level_filter = level;
        self
    }

    pub fn filtered<Q>(mut self, mut f: impl FnMut(&K) -> bool) -> Self
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.items.retain(|d| f(&d.name));
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
            name_display,
            level_filter,
        } = self;

        let items = items
            .into_iter()
            .flat_map(|Diagnostics { text, name, issues }| {
                Self::grouped(level_filter, issues)
                    .into_iter()
                    .map(|(level, issues)| {
                        let name = name_display(&name);
                        (level, Diagnostics { text, name, issues })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .tap_mut(|items| {
                items.sort_by(|(l1, d1), (l2, d2)| (l2, &d1.name).cmp(&(l1, &d2.name)))
            })
            .into_iter()
            .map(|(_, d)| d)
            .collect();

        Reporter { items }
    }

    fn grouped(max: LevelFilter, issues: Vec<P>) -> BTreeMap<Level, Vec<P>> {
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for item in issues {
            let level = item.issue().level();
            if level > max {
                continue;
            }
            groups.entry(level).or_default().push(item);
        }
        groups
    }
}

pub struct Reporter<'a, P> {
    items: Vec<Diagnostics<'a, String, P>>,
}

impl<P> Reporter<'_, P>
where
    P: IssueItem,
{
    pub fn to_level(&self) -> Option<Level> {
        self.items.iter().map(|p| p.status().level()).min()
    }

    pub fn to_stderr(&self) -> &Self {
        if self.items.is_empty() {
            return self;
        }

        if is_logging() {
            self.to_traces();
        } else {
            write!(stderr(), "\n{}", self.to_report())
                .tap_err(emit_debug!())
                .ok();
            if let Some(level) = self.to_level() {
                // explicitly set severity because graphical reports
                // do not go through tracing
                put_severity(level);
            }
        };

        self
    }

    pub fn to_report(&self) -> String {
        self.items.iter().fold(String::new(), |mut out, diag| {
            writeln!(out, "{}", diag.to_report()).expect_fmt();
            out
        })
    }

    pub fn to_traces(&self) {
        for item in self.items.iter() {
            item.to_traces();
        }
    }
}

const fn level_style(level: Level) -> Style {
    match level {
        Level::TRACE => Style::new().dimmed(),
        Level::DEBUG => Style::new().blue(),
        Level::INFO => Style::new().green(),
        Level::WARN => Style::new().yellow(),
        Level::ERROR => Style::new().red(),
    }
}

/// [LevelFilter::current] always returns `TRACE` for some reason
fn max_level() -> LevelFilter {
    if tracing::enabled!(Level::TRACE) {
        LevelFilter::TRACE
    } else if tracing::enabled!(Level::DEBUG) {
        LevelFilter::DEBUG
    } else if tracing::enabled!(Level::INFO) {
        LevelFilter::INFO
    } else if tracing::enabled!(Level::WARN) {
        LevelFilter::WARN
    } else {
        LevelFilter::ERROR
    }
}

trait StyleCompat {
    fn stderr(self) -> Self;
}

impl StyleCompat for Style {
    fn stderr(self) -> Self {
        if is_colored() { self } else { Style::new() }
    }
}

pub trait Title: fmt::Display + Send + Sync {}

impl<K: fmt::Display + Send + Sync> Title for K {}

#[macro_export]
macro_rules! plural {
    ( $num:expr, $singular:expr ) => {
        $crate::plural!($num, $singular, concat!($singular, "s"))
    };
    ( $num:expr, $singular:expr, $plural:expr ) => {{
        let num = $num;
        match num {
            1 => format!("{num} {}", $singular),
            _ => format!("{num} {}", $plural),
        }
    }};
}
