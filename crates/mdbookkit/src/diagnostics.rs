use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt::{Arguments, Debug, Display},
    io::Write,
    ops::Range,
};

pub use annotate_snippets::AnnotationKind;
use annotate_snippets::{
    Annotation, Group, Message, Patch, Renderer, Snippet, renderer::DecorStyle,
};
use bon::Builder;
use tap::{Pipe, TapFallible};

use crate::{emit_debug, env::is_logging, error::put_severity, logging::stderr};

#[derive(Builder, Debug)]
#[builder(start_fn = level)]
pub struct IssueReport<'a> {
    #[builder(start_fn)]
    level: IssueLevel,
    #[builder(into)]
    title: Cow<'a, str>,
    #[builder(default)]
    annotations: Vec<Highlight<'a>>,
    #[builder(default)]
    patches: Vec<Suggestion<'a>>,
    #[builder(default)]
    notes: Vec<Note<'a>>,
    #[builder(default)]
    secondary: Vec<IssueReport<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IssueLevel {
    Error,
    Warning,
    Info,
    Help,
    Note,
}

#[derive(Builder, Debug)]
#[builder(start_fn = span)]
pub struct Highlight<'a> {
    #[builder(start_fn)]
    span: Range<usize>,
    kind: AnnotationKind,
    #[builder(into)]
    label: Option<Cow<'a, str>>,
}

#[derive(Builder, Debug)]
#[builder(start_fn = span)]
pub struct Suggestion<'a> {
    #[builder(start_fn)]
    span: Range<usize>,
    #[builder(into)]
    repl: Cow<'a, str>,
}

#[derive(Builder, Debug)]
#[builder(start_fn = level)]
pub struct Note<'a> {
    #[builder(start_fn)]
    level: IssueLevel,
    #[builder(into)]
    message: Cow<'a, str>,
}

pub struct IssueReporter<'a> {
    pub issues: Vec<IssueReport<'a>>,
    pub source: SourceCode<'a>,
    pub tracer: &'a dyn Fn(IssueLevel, Arguments<'_>),
}

#[derive(Clone)]
pub struct SourceCode<'a> {
    pub source_code: &'a str,
    pub source_path: Cow<'a, str>,
}

impl<'a> IssueReporter<'a> {
    pub fn emit(self) {
        if is_logging() {
            for issue in self.issues {
                issue_to_traces(issue, self.source.clone(), self.tracer);
            }
        } else {
            let renderer = Renderer::styled().decor_style(DecorStyle::Unicode);
            for issue in (self.issues)
                .into_iter()
                .filter(|issue| tracing_level_enabled(issue.level.into()))
            {
                put_severity(issue.level.into());
                let report = issue_to_report(issue, self.source.clone());
                writeln!(stderr(), "{}\n", renderer.render(&report))
                    .tap_err(emit_debug!())
                    .ok();
            }
        }
    }
}

pub fn issue_to_report<'a>(issue: IssueReport<'a>, source: SourceCode<'a>) -> Vec<Group<'a>> {
    macro_rules! snippet {
        ($items:ident, $kind:ident) => {{
            let mut is_empty = true;
            Snippet::source(source.source_code)
                .path(source.source_path.clone())
                .$kind(($items.$kind.into_iter().map(<_>::into)).inspect(|_| is_empty = false))
                .pipe(|v| if is_empty { None } else { Some(v) })
        }};
    }

    let mut sections = Vec::with_capacity(1 + issue.secondary.len());

    let primary = annotate_snippets::Level::from(issue.level)
        .primary_title(issue.title)
        .elements(snippet!(issue, annotations))
        .elements(snippet!(issue, patches))
        .elements(issue.notes.into_iter().map(Message::from));

    sections.push(primary);

    for issue in issue.secondary {
        let secondary = annotate_snippets::Level::from(issue.level)
            .secondary_title(issue.title)
            .elements(snippet!(issue, annotations))
            .elements(snippet!(issue, patches))
            .elements(issue.notes.into_iter().map(Message::from));

        sections.push(secondary);
    }

    sections
}

pub fn issue_to_traces<'a, F>(issue: IssueReport<'a>, source: SourceCode<'a>, emit: F)
where
    F: Fn(IssueLevel, Arguments<'_>),
{
    struct IssueFormatter<'a> {
        issue: IssueReport<'a>,
        source: SourceCode<'a>,
    }

    impl<'a> Display for IssueFormatter<'a> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { issue, source } = self;

            let IssueReport {
                title, annotations, ..
            } = issue;

            let path = &source.source_path;

            if let Some(Highlight { span, label, .. }) = annotations
                .iter()
                .find(|anno| matches!(anno.kind, AnnotationKind::Primary))
                && let Some((line, col)) = byte_to_line_col(source.source_code, span.start)
            {
                write!(f, "{path}:{line}:{col}: {title}")?;
                if let Some(label) = label {
                    write!(f, ": {label}")?;
                }
            } else {
                write!(f, "{path}: {title}")?;
            }

            Ok(())
        }
    }

    let level = issue.level;
    let formatter = IssueFormatter { issue, source };
    emit(level, format_args!("{formatter}"))
}

impl<'a> IssueReport<'a> {
    pub fn secondary(&mut self, item: IssueReport<'a>) -> &mut Self {
        self.secondary.push(item);
        self
    }

    pub fn note(&mut self, note: Note<'a>) -> &mut Self {
        self.notes.push(note);
        self
    }
}

impl<'a> IssueReport<'a> {
    fn sort_key(&self) -> impl Ord + use<> {
        let span = self
            .annotations
            .iter()
            .map(|anno| (anno.span.start, anno.span.end))
            .next();
        (self.level, span)
    }
}

impl<'a> IssueReporter<'a> {
    pub fn sorted(issues: Vec<Self>) -> Vec<Self> {
        let mut sorted = vec![];

        for Self {
            issues,
            source,
            tracer,
        } in issues
        {
            let mut levels = BTreeMap::<_, Vec<_>>::new();
            for issue in issues {
                let level = tracing::Level::from(issue.level);
                levels.entry(level).or_default().push(issue);
            }
            for (level, mut issues) in levels {
                issues.sort_by_key(|issue| issue.sort_key());
                sorted.push((level, issues, source.clone(), tracer));
            }
        }

        sorted.sort_by(|(l1, _, s1, _), (l2, _, s2, _)| {
            (l2, &s1.source_path).cmp(&(l1, &s2.source_path))
        });

        sorted
            .into_iter()
            .map(|(_, issues, source, tracer)| Self {
                issues,
                source,
                tracer,
            })
            .collect()
    }
}

impl<'a> From<Highlight<'a>> for Annotation<'a> {
    fn from(this: Highlight<'a>) -> Self {
        let Highlight { span, kind, label } = this;
        let highlight = matches!(this.kind, AnnotationKind::Primary);
        kind.span(span).label(label).highlight_source(highlight)
    }
}

impl<'a> From<Suggestion<'a>> for Patch<'a> {
    fn from(this: Suggestion<'a>) -> Self {
        Patch::new(this.span, this.repl)
    }
}

impl<'a> From<Note<'a>> for Message<'a> {
    fn from(this: Note<'a>) -> Self {
        annotate_snippets::Level::from(this.level).message(this.message)
    }
}

impl From<IssueLevel> for annotate_snippets::Level<'static> {
    fn from(value: IssueLevel) -> Self {
        match value {
            IssueLevel::Error => annotate_snippets::Level::ERROR,
            IssueLevel::Warning => annotate_snippets::Level::WARNING,
            IssueLevel::Info => annotate_snippets::Level::INFO,
            IssueLevel::Note => annotate_snippets::Level::NOTE,
            IssueLevel::Help => annotate_snippets::Level::HELP,
        }
    }
}

impl From<IssueLevel> for tracing::Level {
    fn from(value: IssueLevel) -> Self {
        match value {
            IssueLevel::Error => tracing::Level::ERROR,
            IssueLevel::Warning => tracing::Level::WARN,
            IssueLevel::Info => tracing::Level::INFO,
            IssueLevel::Note => tracing::Level::DEBUG,
            IssueLevel::Help => tracing::Level::INFO,
        }
    }
}

impl Display for IssueLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueLevel::Error => f.write_str("error"),
            IssueLevel::Warning => f.write_str("warning"),
            IssueLevel::Info => f.write_str("info"),
            IssueLevel::Note => f.write_str("note"),
            IssueLevel::Help => f.write_str("help"),
        }
    }
}

#[macro_export]
macro_rules! emit_issue {
    () => {
        &|level: $crate::diagnostics::IssueLevel, message: ::std::fmt::Arguments<'_>| match level {
            $crate::diagnostics::IssueLevel::Error => ::tracing::error!("{message}"),
            $crate::diagnostics::IssueLevel::Warning => ::tracing::warn!("{message}"),
            $crate::diagnostics::IssueLevel::Info => ::tracing::info!("{message}"),
            $crate::diagnostics::IssueLevel::Note => ::tracing::debug!("{message}"),
            $crate::diagnostics::IssueLevel::Help => ::tracing::info!("{message}"),
        }
    };
}

fn tracing_level_enabled(level: tracing::Level) -> bool {
    if tracing::enabled!(tracing::Level::TRACE) {
        level <= tracing::Level::TRACE
    } else if tracing::enabled!(tracing::Level::DEBUG) {
        level <= tracing::Level::DEBUG
    } else if tracing::enabled!(tracing::Level::INFO) {
        level <= tracing::Level::INFO
    } else if tracing::enabled!(tracing::Level::WARN) {
        level <= tracing::Level::WARN
    } else {
        level <= tracing::Level::ERROR
    }
}

impl<'a, P: Display> From<(&'a str, P)> for SourceCode<'a> {
    fn from((source_code, source_path): (&'a str, P)) -> Self {
        let source_path = source_path.to_string().into();
        Self {
            source_code,
            source_path,
        }
    }
}

fn byte_to_line_col(text: &str, byte: usize) -> Option<(usize, usize)> {
    if byte >= text.len() {
        return None;
    }
    let mut scanned = 0;
    for (line, text) in text.split('\n').enumerate() {
        if scanned + text.len() >= byte {
            let mut count = 0;
            for (column, (ch, _)) in text.char_indices().enumerate() {
                if scanned + ch >= byte {
                    return Some((line + 1, column + 1));
                } else {
                    count = ch;
                }
            }
            return Some((line + 1, count + 1));
        } else {
            scanned += text.len() + '\n'.len_utf8();
        }
    }
    None
}
