use std::ops::Range;

use cargo_metadata::diagnostic::{
    Diagnostic,
    DiagnosticLevel::{self, *},
    DiagnosticSpan,
};
use tap::Pipe;

use mdbookkit::diagnostics::{
    Highlight, IssueLevel, IssueReport, Note, Suggestion, annotate_snippets::AnnotationKind,
};

pub struct RustcDiagnostic<'a, 'r> {
    pub diagnostic: &'a Diagnostic,
    pub source_map: &'r (dyn SourceMap + 'r),
}

pub trait SourceMap {
    fn map_span(&self, span: &DiagnosticSpan) -> Option<Range<usize>>;
}

impl<'a, 'r> From<RustcDiagnostic<'a, 'r>> for IssueReport<'a> {
    fn from(this: RustcDiagnostic<'a, 'r>) -> Self {
        let RustcDiagnostic {
            diagnostic,
            source_map,
        } = this;

        let annotations = (diagnostic.spans.iter())
            .filter(|item| item.suggested_replacement.is_none())
            .filter_map(|item| {
                Highlight::span(source_map.map_span(item)?)
                    .kind(if item.is_primary {
                        AnnotationKind::Primary
                    } else {
                        AnnotationKind::Context
                    })
                    .maybe_label(item.label.as_ref())
                    .build()
                    .pipe(Some)
            })
            .collect();

        let patches = (diagnostic.spans.iter())
            .filter_map(|item| {
                let repl = item.suggested_replacement.as_ref()?;
                let span = source_map.map_span(item)?;
                Some(Suggestion::span(span).repl(repl).build())
            })
            .collect();

        let notes = (diagnostic.children.iter())
            .filter(|item| is_note(item))
            .filter(|item| !less_helpful_message(item))
            .map(|item| {
                Note::level(report_level(item.level))
                    .message(&item.message)
                    .build()
            })
            .collect();

        let secondary = (diagnostic.children.iter())
            .filter(|item| !is_note(item))
            .map(|diagnostic| {
                RustcDiagnostic {
                    diagnostic,
                    source_map,
                }
                .into()
            })
            .collect();

        IssueReport::level(report_level(diagnostic.level))
            .title(&diagnostic.message)
            .annotations(annotations)
            .patches(patches)
            .notes(notes)
            .secondary(secondary)
            .build()
    }
}

fn report_level(level: DiagnosticLevel) -> IssueLevel {
    match level {
        Error => IssueLevel::Error,
        Warning => IssueLevel::Warning,
        Note => IssueLevel::Note,
        Help => IssueLevel::Help,
        FailureNote => IssueLevel::Note,
        Ice => IssueLevel::Error,
        _ => IssueLevel::Error,
    }
}

fn is_note(diagnostic: &Diagnostic) -> bool {
    diagnostic.spans.is_empty()
}

fn less_helpful_message(message: &Diagnostic) -> bool {
    let Diagnostic { message, level, .. } = message;
    match level {
        DiagnosticLevel::Note => message.starts_with(r"`#[warn("),
        DiagnosticLevel::Help => message.starts_with(r"to escape `[` and `]` characters"),
        _ => false,
    }
}
