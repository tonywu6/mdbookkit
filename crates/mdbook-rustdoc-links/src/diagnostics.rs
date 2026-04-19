use std::{collections::BTreeSet, ops::Range};

use anyhow::Context;
use cargo_metadata::diagnostic::{
    Diagnostic,
    DiagnosticLevel::{self, *},
    DiagnosticSpan,
};
use tap::Pipe;

use mdbookkit::diagnostics::{
    Highlight, IssueLevel, IssueReport, Note, Suggestion, annotate_snippets::AnnotationKind,
};

use crate::options::{BuildOptions, CargoOptions, FeatureSelection};

pub struct RustcDiagnostic<'a, 'r> {
    pub diagnostic: &'a Diagnostic,
    pub source_map: &'r (dyn SourceMap + 'r),
}

pub trait SourceMap {
    fn map_span(&self, span: &DiagnosticSpan) -> Option<Range<usize>>;
    fn include_note(&self, diag: &Diagnostic, note: &str) -> bool;
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
            .filter(|item| source_map.include_note(diagnostic, &item.message))
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

pub fn report_level(level: DiagnosticLevel) -> IssueLevel {
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

#[derive(Debug, Default, Clone)]
pub struct DiagnosticNotes {
    options_specified: BTreeSet<&'static str>,
    preludes_derived: Vec<String>,
    preludes_not_derived: Option<&'static str>,
    visited: VisitedNotes,
}

#[derive(Debug, Default, Clone)]
struct VisitedNotes {
    options_specified: bool,
    preludes_derived: bool,
    preludes_not_derived: bool,
}

impl DiagnosticNotes {
    pub fn note_options_specified(&mut self) -> Option<String> {
        if self.visited.options_specified {
            return None;
        }
        self.visited.options_specified = true;
        let options = self.print_specified_options()?;
        let note = format! {
            "the following options have been specified, which \
            may have affected link resolution:\n{options}",
        };
        Some(note)
    }

    fn print_specified_options(&self) -> Option<String> {
        if self.options_specified.is_empty() {
            return None;
        }
        self.options_specified
            .iter()
            .map(|opt| format!("- {opt}"))
            .collect::<Vec<_>>()
            .join("\n")
            .pipe(Some)
    }

    pub fn note_preludes_derived(&mut self) -> Option<String> {
        if self.preludes_derived.is_empty() || self.visited.preludes_derived {
            return None;
        }
        self.visited.preludes_derived = true;
        let preludes = self
            .preludes_derived
            .iter()
            .map(|module| format!("use {module};"))
            .collect::<Vec<_>>()
            .join("\n");
        let note = format! {
            "in order to resolve links, the preprocessor creates a temporary crate;\n\
            the following prelude has been implicitly added to the temporary crate:\n`{preludes}`"
        };
        Some(note)
    }

    pub fn note_preludes_not_derived(&mut self) -> Option<String> {
        if self.visited.preludes_not_derived {
            return None;
        }
        self.visited.preludes_not_derived = true;
        let reason = self.preludes_not_derived?;
        let note = format! {
            "in order to resolve links, the preprocessor creates a temporary crate;\n\
            a prelude was not implicitly added to the temporary crate because:\n{reason}"
        };
        Some(note)
    }

    pub fn mark_option_specified(&mut self, options: &BuildOptions) {
        let BuildOptions {
            packages,
            preludes,
            features,
            rustc_args,
            rustdoc_args,
            cargo,
            docs_rs,
        } = options;
        if !packages.is_empty() {
            self.options_specified.insert("build.packages");
        }
        if preludes.is_some() {
            self.options_specified.insert("build.preludes");
        }
        {
            let FeatureSelection {
                features,
                all_features,
                no_default_features,
            } = features;
            if !features.is_empty() {
                self.options_specified.insert("build.features");
            }
            if all_features.is_some() {
                self.options_specified.insert("build.all-features");
            }
            if no_default_features.is_some() {
                self.options_specified.insert("build.no-default-features");
            }
        }
        if !rustc_args.is_empty() {
            self.options_specified.insert("build.rustc-args");
        }
        if !rustdoc_args.is_empty() {
            self.options_specified.insert("build.rustdoc-args");
        }
        {
            let CargoOptions {
                toolchain,
                cargo_args,
                runner,
            } = cargo;
            if toolchain.is_some() {
                self.options_specified.insert("build.toolchain");
            }
            if !cargo_args.is_empty() {
                self.options_specified.insert("build.cargo-args");
            }
            if !runner.is_undefined() {
                self.options_specified.insert("build.runner");
            }
        }
        if docs_rs.is_some() {
            self.options_specified.insert("build.docs-rs");
        }
    }

    pub fn mark_preludes_derived(&mut self, preludes: Vec<String>) -> Vec<String> {
        self.preludes_derived = preludes.clone();
        preludes
    }

    pub fn mark_preludes_not_derived_because(&mut self, reason: &'static str) {
        self.preludes_not_derived = Some(reason)
    }
}

pub trait ErrorWithNotes<T> {
    fn note_options(self, hints: &DiagnosticNotes) -> anyhow::Result<T>;
}

impl<T> ErrorWithNotes<T> for anyhow::Result<T> {
    fn note_options(self, hints: &DiagnosticNotes) -> anyhow::Result<T> {
        if let Some(options) = hints.print_specified_options() {
            let note = format! { "possibly the following options that \
            have been specified:\n{options}" };
            self.context(note)
        } else {
            self
        }
    }
}
