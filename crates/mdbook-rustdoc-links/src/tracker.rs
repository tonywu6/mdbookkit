use std::{
    cell::RefCell,
    collections::BTreeSet,
    fmt::{Debug, Display, Write},
    ops::{ControlFlow, Range},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{
    Package,
    diagnostic::{Diagnostic, DiagnosticSpan},
};
use html_escape::decode_html_entities;
use lol_html::{HtmlRewriter, element, text};
use mdbook_markdown::pulldown_cmark::{
    CowStr, Event,
    LinkType::{self, *},
    Tag, TagEnd,
};
use percent_encoding::percent_decode_str;
use pulldown_cmark_to_cmark::cmark_resume;
use tap::{Conv, Pipe};
use tracing::{debug, instrument, trace, warn};
use url::Url;

use mdbookkit::{
    cmp::{Lexicographic, LexicographicOrd},
    diagnostics::{
        Highlight, IssueLevel, IssueReport, Note, Suggestion, annotate_snippets::AnnotationKind,
    },
    emit_debug, emit_warning,
    error::{Break, ExpectFmt},
    markdown::PatchStream,
    plural, with_bug_report,
};

use crate::{
    builder::BuildOutput,
    diagnostics::{DiagnosticNotes, RustcDiagnostic, SourceMap, report_level},
    markdown::markdown,
};

#[derive(Debug, Default)]
pub struct LinkTracker<'a> {
    links: Vec<Link<'a>>,
    pages: Vec<Page<'a>>,
    notes: DiagnosticNotes,
}

#[derive(Debug)]
struct Page<'a> {
    text: &'a str,
    link_end: usize,
}

#[derive(Debug)]
struct Link<'a> {
    href: Option<Url>,
    kind: LinkType,
    span: SourceSpan,
    dest: CowStr<'a>,
    title: CowStr<'a>,
    inner_elem: Vec<Event<'a>>,
    normalized: NormalizedLink<'a>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
enum NormalizedLink<'a> {
    Unmodified { text: &'a str },
    Normalized { text: String, span: SourceSpan },
}

#[derive(Debug, Clone)]
struct SourceSpan {
    full: Range<usize>,
    text: Range<usize>,
    dest: Range<usize>,
}

impl<'a> LinkTracker<'a> {
    pub fn read(&mut self, text: &'a str) -> Result<()> {
        let mut state = None;

        for (event, span) in markdown(text).into_offset_iter() {
            match state.take() {
                None => {
                    state = Link::try_open(text, event, span);
                }
                Some(mut link) => match link.push(event, span)? {
                    ControlFlow::Continue(()) => {
                        state = Some(link);
                    }
                    ControlFlow::Break(()) => {
                        if let Ok(link) = link.normalized() {
                            self.links.push(link);
                        }
                    }
                },
            }
        }

        self.pages.push(Page {
            text,
            link_end: self.links.len(),
        });

        Ok(())
    }

    pub fn rustdoc_input(&self) -> Option<String> {
        let mut input = String::new();
        let mut empty = true;

        for link in self.links.iter() {
            let Link {
                href, normalized, ..
            } = link;
            if href.is_none() {
                empty = false;
                let link = normalized.as_ref();
                writeln!(input, "{COMMENT_PREFIX}{link}{COMMENT_SUFFIX}")
            } else {
                writeln!(input, "{COMMENT_PREFIX}{COMMENT_SUFFIX}")
            }
            .expect_fmt();
        }

        debug!("rustdoc input:\n{input}");

        if empty { None } else { Some(input) }
    }

    #[instrument("debug", skip_all)]
    pub fn rustdoc_output(&mut self, output: BuildOutput<'_>) {
        let BuildOutput {
            ref stdout,
            ref stderr,
            ..
        } = output;

        struct State<'r, 'a> {
            links: &'r mut Vec<Link<'a>>,
            row: Option<usize>,
            text_content: String,
        }

        let state = RefCell::new(State {
            links: &mut self.links,
            row: None,
            text_content: String::new(),
        });

        lol_html::Settings {
            element_content_handlers: vec![
                element!(OUTER_SELECTOR, |_| {
                    state.borrow_mut().enter();
                    Ok(())
                }),
                element!("a[href]", |elem| {
                    if !state.borrow().has_link() {
                        return Ok(());
                    };
                    trace!(concat!("abc", env!("CARGO_PKG_REPOSITORY")));
                    trace!("{elem:?}");

                    if let Some(href) = elem.get_attribute("href")
                        && let Some(link) = state.borrow_mut().link()
                        && !eq_escaped(&link.dest, &href)
                    {
                        if let Ok(url) = resolve_url(&output, &href)
                            .with_context(|| format!("could not convert to a full URL: {href:?}"))
                            .or_else(with_bug_report!(emit_warning))
                        {
                            link.href = Some(url)
                        }
                        if let Some(title) = elem.get_attribute("title") {
                            link.title = title.into()
                        }
                    }

                    Ok(())
                }),
                text!("a[href]", |text| {
                    if !state.borrow().has_link() {
                        return Ok(());
                    };

                    let text = text.as_str();
                    if text.is_empty() {
                        let mut state = state.borrow_mut();
                        let text = std::mem::take(&mut state.text_content);
                        let link = state.link().expect("checked");
                        if matches!(link.kind, CollapsedUnknown | ShortcutUnknown)
                            && link.inner_elem.len() == 1
                            && let Some(Event::Text(original) | Event::Code(original)) =
                                link.inner_elem.first_mut()
                        {
                            let text = decode_html_entities(&text);
                            *original = CowStr::Boxed(text.into())
                        }
                    } else {
                        state.borrow_mut().text_content.push_str(text);
                    }

                    Ok(())
                }),
            ],
            ..Default::default()
        }
        .pipe(|cb| HtmlRewriter::new(cb, |_: &[u8]| ()))
        .pipe(|mut wr| wr.write(stdout.as_bytes()).and_then(|_| wr.end()))
        .context("unexpected error from HtmlRewriter")
        .or_else(emit_debug!())
        .ok();

        impl<'a> State<'_, 'a> {
            fn enter(&mut self) {
                self.row = match self.row {
                    None => Some(0),
                    Some(i) => Some(i + 1),
                };
                self.text_content = String::new();
            }

            fn link(&mut self) -> Option<&mut Link<'a>> {
                Some(&mut self.links[self.row?])
            }

            fn has_link(&self) -> bool {
                self.row.is_some()
            }
        }

        for line in String::from_utf8_lossy(stderr).lines() {
            if let Ok(diag) = serde_json::from_str::<Diagnostic>(line)
                .with_context(|| line.to_owned())
                .context("could not parse line as diagnostic")
                .or_else(emit_debug!())
                && let Ok(line) = locate_diagnostic(&diag)
                    .with_context(|| format!("{diag:?}"))
                    .context("could not determine primary line")
                    .or_else(emit_debug!())
                && let Ok(link) = (self.links.get_mut(line))
                    .with_context(|| format!("{diag:?}"))
                    .with_context(|| format!("line {line}"))
                    .context("line does not belong to any link")
                    .or_else(emit_debug!())
            {
                trace! {
                    "line {}, link {:?}, {}", line + 1, &*link.dest,
                    diag.rendered.as_deref().unwrap_or(&diag.message)
                };
                link.diagnostics.push(diag);
            }
        }
    }

    pub fn notes(&mut self) -> &mut DiagnosticNotes {
        &mut self.notes
    }

    pub fn export<'d: 'a>(&'d self) -> ExportedPages<'d> {
        let mut contents = Vec::with_capacity(self.pages.len());
        let mut issues = Vec::with_capacity(self.pages.len());

        let links = self.pages.iter().scan(0usize, |start, page| {
            let links = &self.links[*start..page.link_end];
            *start = page.link_end;
            Some((page, links))
        });

        let mut ctx = IssueReportContext {
            tracker: self,
            notes: self.notes.clone(),
        };
        let mut stats = Statistics::default();

        for (page, links) in links {
            let page_issues = links
                .iter()
                .inspect(|link| stats.count(link))
                .flat_map(|link| ctx.diagnose(link))
                .chain(self.link_summary(links))
                .collect();

            issues.push(page_issues);

            let patches = links.iter().filter_map(|link| {
                let span = link.span.full.clone();
                let link = link.export()?;
                Some((link, span))
            });

            let text = PatchStream::new(page.text, patches)
                .into_string()
                .map_err(<_>::into);

            contents.push(text);
        }

        ExportedPages {
            contents,
            issues,
            stats,
        }
    }

    fn link_summary(&self, links: &'a [Link<'a>]) -> Option<IssueReport<'a>> {
        let resolved = links
            .iter()
            .filter_map(|link| {
                Highlight::span(link.span.dest.clone())
                    .kind(AnnotationKind::Primary)
                    .label(link.href.as_ref()?.as_str())
                    .build()
                    .pipe(Some)
            })
            .collect::<Vec<_>>();

        if resolved.is_empty() {
            return None;
        }

        IssueReport::level(IssueLevel::Note)
            .title(format!("{} resolved", plural!(resolved.len(), "link")))
            .annotations(resolved)
            .build()
            .pipe(Some)
    }
}

pub struct ExportedPages<'a> {
    pub contents: Vec<Result<String>>,
    pub issues: Vec<Vec<IssueReport<'a>>>,
    pub stats: Statistics,
}

macro_rules! link_class {
    (href_defined) => {
        Inline | Reference | Collapsed | Shortcut
    };
    (ignored_link) => {
        Autolink | Email | WikiLink { .. }
    };
}

fn could_be_item_link(kind: LinkType, dest: &str) -> bool {
    if matches!(kind, link_class!(ignored_link)) {
        return false;
    }
    if matches!(kind, link_class!(href_defined)) && dest.contains('/') {
        return false;
    }

    let dest = if let Some(idx) = dest.find('#') {
        &dest[..idx]
    } else {
        dest
    };

    if dest.is_empty() {
        return false;
    }

    if matches!(kind, ShortcutUnknown)
        && let Some(suffix) = dest.strip_prefix('!')
        && !suffix.is_empty()
        && suffix.chars().all(|c| c.is_ascii_alphabetic())
    {
        return false;
    }

    true
}

fn locate_text(source: &str, sliced: &str, fallback: &Range<usize>) -> Range<usize> {
    let sliced_lower = sliced.as_ptr();
    let sliced_upper = unsafe { sliced_lower.add(sliced.len()) };
    let source_lower = source.as_ptr();
    let source_upper = unsafe { source_lower.add(source.len()) };
    if source_lower <= sliced_lower && sliced_upper <= source_upper {
        let lower = unsafe { sliced_lower.offset_from_unsigned(source_lower) };
        let upper = unsafe { sliced_upper.offset_from_unsigned(source_lower) };
        lower..upper
    } else {
        fallback.clone()
    }
}

fn eq_escaped(original: &str, encoded: &str) -> bool {
    let decoded = match percent_decode_str(encoded).decode_utf8() {
        Ok(decoded) => decoded,
        Err(..) => return false,
    };
    let decoded = decode_html_entities(&decoded);
    original == decoded
}

fn resolve_url(output: &BuildOutput<'_>, href: &str) -> Result<Url> {
    if let Ok(url) = href.parse() {
        return Ok(url);
    }

    let path = if let Some(href) = href.strip_prefix("../")
        && let Some((lib, _)) = href.split_once('/')
        && let Some(package) = output.crates.get(lib)
    {
        let Package { name, version, .. } = &output.metadata[package];
        format!("/{name}/{version}/{href}")
    } else {
        bail!("unsupported link format")
    };

    Ok("https://docs.rs".parse::<Url>()?.join(&path)?)
}

fn locate_diagnostic(diag: &Diagnostic) -> Option<usize> {
    diag.spans.iter().find_map(|span| {
        if span.file_name == "<anon>" && span.is_primary {
            locate_diagnostic_span(span)
        } else {
            None
        }
    })
}

fn locate_diagnostic_span(span: &DiagnosticSpan) -> Option<usize> {
    if span.line_start == span.line_end {
        Some(span.line_start - 1)
    } else {
        None
    }
}

macro_rules! data_attr {
    () => {
        env!("CARGO_PKG_NAME")
    };
}
static OUTER_SELECTOR: &str = concat!("span[", data_attr!(), "]");
static COMMENT_PREFIX: &str = concat!("//! - <span ", data_attr!(), ">");
static COMMENT_SUFFIX: &str = "</span>";

impl<'a> Link<'a> {
    fn try_open(text: &'a str, event: Event<'a>, span: Range<usize>) -> Option<Self> {
        let Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            ..
        }) = event
        else {
            return None;
        };

        if !could_be_item_link(link_type, &dest_url) {
            trace!(?span, dest = ?&*dest_url, "link ...");
            return None;
        }

        trace!(?span, dest = ?&*dest_url, title = ?&*title, "link >>>");
        Some(Link {
            href: None,
            kind: link_type,
            span: SourceSpan {
                full: span.clone(),
                text: text.len()..0, // empty span
                dest: locate_text(text, &dest_url, &span),
            },
            dest: dest_url,
            title,
            inner_elem: Default::default(),
            normalized: NormalizedLink::borrowed(&text[span]),
            diagnostics: Default::default(),
        })
    }

    fn push(&mut self, event: Event<'a>, span: Range<usize>) -> Result<ControlFlow<()>> {
        match event {
            Event::Start(Tag::Link { .. }) => {
                debug!(?span, "unexpected `Tag::Link` in `Tag::Link`");
                bail!("markdown stream malformed at {span:?}");
            }

            Event::End(TagEnd::Link) => {
                if self.span.full == span {
                    trace!(?span, "link <<<");

                    if self.span.text.is_empty() {
                        self.span.text = self.span.full.clone();
                    }

                    Ok(ControlFlow::Break(()))
                } else {
                    debug!(?span, "mismatching span, expected {:?}", self.span.full);
                    bail!("markdown stream malformed at {span:?}");
                }
            }

            event => {
                trace!(?span, ?event, parent = ?self.span.full, "link +++");

                self.inner_elem.push(event);

                if self.span.text.start > span.start {
                    self.span.text.start = span.start
                }
                if self.span.text.end < span.end {
                    self.span.text.end = span.end
                }

                Ok(ControlFlow::Continue(()))
            }
        }
    }

    #[instrument(level = "debug", skip_all, fields(link = ?&*self.dest, span = ?self.span.full))]
    fn normalized(mut self) -> Result<Self, Break> {
        let Self {
            kind,
            dest,
            title,
            inner_elem,
            normalized,
            ..
        } = &self;

        let is_shortcut = if matches!(kind, CollapsedUnknown | ShortcutUnknown)
            && inner_elem.len() == 1
            && let Some(Event::Text(text) | Event::Code(text)) = inner_elem.first()
            && text == dest
        {
            true
        } else {
            false
        };

        let is_one_line = match kind {
            CollapsedUnknown | ShortcutUnknown => is_shortcut,
            Inline | ReferenceUnknown => true,
            Reference | Collapsed | Shortcut => false,
            link_class!(ignored_link) => unreachable!(),
        } && !normalized.as_ref().contains('\n');

        if is_one_line {
            return Ok(self);
        }

        // https://spec.commonmark.org/0.31.2/#link-title
        // > link titles may span multiple lines
        let title = title.replace('\n', "&#10;").into();

        let link = match kind {
            ReferenceUnknown | CollapsedUnknown | ShortcutUnknown => Tag::Link {
                link_type: Reference,
                dest_url: "".into(),
                title,
                id: dest.clone(),
            },
            link_class!(href_defined) => Tag::Link {
                link_type: Inline,
                dest_url: dest.clone(),
                title,
                id: "".into(),
            },
            link_class!(ignored_link) => {
                unreachable!()
            }
        };

        let events = ([Event::Start(link)].into_iter())
            .chain(inner_elem.clone())
            .chain([Event::End(TagEnd::Link)]);

        let (text, span) = (|| -> Result<_> {
            let mut text = String::with_capacity(normalized.as_ref().len());

            // dropping the state because we are not
            // appending the final link definition
            let _ = cmark_resume(events, &mut text, None)?;

            let span = {
                let mut state = None;
                for (event, span) in markdown(&text).into_offset_iter() {
                    match state.as_mut() {
                        None => {
                            state = Link::try_open(&text, event, span);
                        }
                        Some(link) => {
                            if let ControlFlow::Break(()) = link.push(event, span)? {
                                break;
                            }
                        }
                    }
                }
                state.context("should have parsed a link")?.span
            };

            Ok((text, span))
        })()
        .context("internal error while parsing link; the link will be skipped")
        .or_else(emit_warning!())?;

        // link text may still contain newlines
        let text = text.replace('\n', " ");
        // this is acceptable even within `inline code`:
        // https://spec.commonmark.org/0.31.2/#code-spans

        self.normalized = NormalizedLink::Normalized { text, span };

        Ok(self)
    }

    fn export(&'a self) -> Option<impl Iterator<Item = Event<'a>>> {
        let Self {
            href,
            title,
            inner_elem,
            ..
        } = self;

        let href = href.as_ref()?;

        let iter = std::iter::once(Event::Start(Tag::Link {
            link_type: Inline,
            dest_url: CowStr::Borrowed(href.as_str()),
            title: title.clone(),
            id: CowStr::Borrowed(""),
        }))
        .chain(inner_elem.iter().cloned())
        .chain(std::iter::once(Event::End(TagEnd::Link)));

        Some(iter)
    }
}

struct IssueReportContext<'a> {
    tracker: &'a LinkTracker<'a>,
    notes: DiagnosticNotes,
}

impl<'a> IssueReportContext<'a> {
    fn diagnose(&mut self, link: &'a Link<'a>) -> Vec<IssueReport<'a>> {
        let mut issues = Vec::with_capacity(link.diagnostics.len());
        let mut seen = BTreeSet::new();

        if link.href.is_none() && link.diagnostics.is_empty() {
            let span = &link.span.dest;

            let issue = IssueReport::level(IssueLevel::Note)
                .title("link ignored")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Context)
                        .build(),
                ])
                .build();

            issues.push(issue);
        }

        for diagnostic in link.diagnostics.iter() {
            if seen
                .replace(Lexicographic(DiagnosticKey(diagnostic)))
                .is_some()
            {
                continue;
            }

            let is_unresolved = has_error_code(diagnostic, "rustdoc::broken_intra_doc_links")
                && diagnostic.message.starts_with("unresolved link to");

            if is_unresolved && link.href.is_some() {
                continue;
            }

            let mut issue = RustcDiagnostic {
                diagnostic,
                source_map: self.tracker,
            }
            .conv::<IssueReport>();

            if is_unresolved {
                self.augment_unresolved(link, &mut issue);
            }

            issues.push(issue);
        }

        issues
    }

    fn augment_unresolved(&mut self, link: &Link<'_>, report: &mut IssueReport<'_>) {
        let could_be_a_path = matches!(link.kind, link_class!(href_defined))
            && !link.dest.contains("::")
            && !link.dest.contains("<");

        if could_be_a_path {
            let span = &link.span.dest;
            let span = span.start..span.start;
            let help = {
                "to indicate that this is a relative path (which will silence this warning),\n\
                prepend the link with `./`"
            };
            let suggestion = IssueReport::level(IssueLevel::Help)
                .title(help)
                .patches(vec![Suggestion::span(span).repl("./").build()])
                .build();
            report.secondary(suggestion);
        }

        let could_be_top_level = report.iter_labels().any(|label| {
            label.ends_with(" in scope") || label.contains(" in module `temporary_crate_")
        });

        if could_be_top_level && let Some(note) = self.notes.note_options_specified() {
            report.note(Note::note(note));
        }

        let specifies_crate = if link.dest.starts_with("crate::") {
            Some("crate")
        } else if link.dest.starts_with("self::") {
            Some("self")
        } else {
            None
        };

        if let Some(specifies_crate) = specifies_crate
            && could_be_top_level
        {
            if let Some(note) = self.notes.note_preludes_derived() {
                report.note(Note::note(note));
            } else {
                let help1 =
                    format!("try specifying the crate name instead of `{specifies_crate}::`");
                let help2 = "or use the `build.preludes` option to introduce this item into scope";
                report.note(Note::help(help1)).note(Note::help(help2));
                if let Some(note) = self.notes.note_preludes_not_derived() {
                    report.note(Note::note(note));
                }
            }
        }
    }
}

fn has_error_code(diag: &Diagnostic, code: &str) -> bool {
    diag.code.as_ref().map(|c| c.code == code).unwrap_or(false)
}

impl SourceMap for LinkTracker<'_> {
    fn map_span(&self, span: &DiagnosticSpan) -> Option<Range<usize>> {
        let link = self.links.get(locate_diagnostic_span(span)?)?;
        let line = span.text.first()?;

        let lower = line.highlight_start - 1;
        let upper = line.highlight_end - 1;

        let lower = line.text.char_indices().nth(lower)?.0;
        let upper = line.text.char_indices().nth(upper)?.0;

        let lower = lower.checked_sub(COMMENT_PREFIX.len())?;
        let upper = upper.checked_sub(COMMENT_PREFIX.len())?;
        let len = upper.checked_sub(lower)?;

        let (span, lower) = match &link.normalized {
            NormalizedLink::Unmodified { .. } => {
                let span = &link.span.full;
                (span, span.start + lower)
            }
            NormalizedLink::Normalized { span, .. } => {
                if span.text.start <= lower && lower <= span.text.end {
                    let source = &link.span.text;
                    let mapped = &span.text;
                    (source, (source.start + lower - mapped.start))
                } else if span.dest.start <= lower && lower <= span.dest.end {
                    let source = &link.span.dest;
                    let mapped = &span.dest;
                    (source, (source.start + lower - mapped.start))
                } else {
                    let span = &link.span.full;
                    (span, span.start)
                }
            }
        };

        let len = len.min(span.end - span.start);
        let upper = lower + len;
        Some(lower..upper)
    }

    fn include_note(&self, diag: &Diagnostic, note: &str) -> bool {
        let link = if let Some(idx) = locate_diagnostic(diag)
            && let Some(link) = self.links.get(idx)
        {
            link
        } else {
            return true;
        };

        if note.starts_with(r"`#[warn(") {
            false
        } else if note.starts_with(r"to escape `[`") {
            matches!(
                link.kind,
                ReferenceUnknown | CollapsedUnknown | ShortcutUnknown
            ) && !matches!(link.inner_elem.as_slice(), [Event::Code(..)])
        } else {
            true
        }
    }
}

impl<'a> NormalizedLink<'a> {
    fn borrowed(text: &'a str) -> Self {
        Self::Unmodified { text }
    }
}

impl AsRef<str> for NormalizedLink<'_> {
    fn as_ref(&self) -> &str {
        match self {
            Self::Normalized { text, .. } => text,
            Self::Unmodified { text } => text,
        }
    }
}

#[derive(Debug)]
struct DiagnosticKey<'a>(&'a Diagnostic);

impl LexicographicOrd for DiagnosticKey<'_> {
    fn head(&self) -> impl Ord {
        (
            report_level(self.0.level),
            self.0.code.as_ref().map(|c| &c.code),
            &self.0.message,
        )
    }

    fn tail(&self) -> impl Iterator<Item = impl Ord> {
        (self.0.spans.iter()).map(|span| {
            (
                &span.is_primary,
                &span.line_start,
                &span.column_start,
                &span.line_end,
                &span.column_end,
                &span.label,
                &span.file_name,
                &span.suggested_replacement,
            )
        })
    }
}

#[derive(Debug, Default)]
pub struct Statistics {
    processed: usize,
    resolved: usize,
    unresolved: usize,
    has_warnings: usize,
    unsupported: usize,
}

impl Statistics {
    fn count(&mut self, link: &Link<'_>) {
        self.processed += 1;
        if link.href.is_some() {
            self.resolved += 1
        } else {
            self.unresolved += 1;
        }
        if !link.diagnostics.is_empty() {
            self.has_warnings += 1
        } else if link.href.is_none() {
            self.unresolved += 1;
        }
    }
}

impl Display for Statistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            processed,
            resolved,
            unresolved,
            has_warnings,
            unsupported,
        } = self;
        write! { f,
            "processed {processed}: \
            {resolved} resolved; \
            {unresolved} unresolved",
            processed = plural!(processed, "link"),
        }?;
        if has_warnings > &0 {
            write! { f, "; {}",
                plural!(has_warnings, "has warnings", "have warnings")
            }?
        }
        if unsupported > &0 {
            write!(f, "; {unsupported} may be unsupported")?
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use mdbookkit::diagnostics::{
        Highlight, IssueLevel, IssueReport,
        annotate_snippets::{AnnotationKind, Renderer, renderer::DecorStyle},
        issue_to_report,
    };
    use mdbookkit_testing::{
        AssertUtil, default_assert,
        snapbox::{Data, data::DataFormat, utils::current_dir},
    };

    use super::{LinkTracker, SourceSpan};

    fn print_link_spans(span: SourceSpan) -> IssueReport<'static> {
        let SourceSpan { full, text, dest } = span;
        IssueReport::level(IssueLevel::Warning)
            .title("link spans")
            .annotations(vec![
                Highlight::span(full)
                    .kind(AnnotationKind::Primary)
                    .label("full")
                    .build(),
                Highlight::span(dest)
                    .kind(AnnotationKind::Context)
                    .label("dest")
                    .build(),
                Highlight::span(text)
                    .kind(AnnotationKind::Context)
                    .label("text")
                    .build(),
            ])
            .build()
    }

    fn test_link_spans(text: &str, expected: Data) -> Result<()> {
        let mut tracker = LinkTracker::default();
        tracker.read(text)?;
        let span = tracker.links[0].span.clone();
        let report = print_link_spans(span);
        let report = issue_to_report(report, (text, "<anon>").into());
        let renderer = Renderer::styled().decor_style(DecorStyle::Ascii);
        let actual = renderer.render(&report);
        default_assert().try_eq_text(None, actual, expected)?;
        Ok(())
    }

    macro_rules! test_link_spans {
        ( $name:ident ( $($line:literal),* ) ) => {
            #[test]
            fn $name() -> Result<()> {
                let path = current_dir!()
                    .join("tracker/tests")
                    .join(concat!(stringify!($name), ".svg"));
                let data = Data::read_from(&path, Some(DataFormat::TermSvg));
                let text = concat!($($line, "\n"),*);
                test_link_spans(text, data)
            }
        };
    }

    test_link_spans!(link_span_inline("[drop](drop)"));
    test_link_spans!(link_span_inline_with_title(
        "[drop](drop 'This function is not magic')"
    ));
    test_link_spans!(link_span_inline_whitespace(
        "[`Infallible`]( std::convert::Infallible",
        "'The error type for errors that can never happen",
        "Since this enum has no variant, a value of this type can never actually exist.')"
    ));

    test_link_spans!(link_span_reference("[drop][drop]", "", "[drop]: drop"));
    test_link_spans!(link_span_reference_with_title(
        "[drop][drop]",
        "",
        "[drop]: drop 'This function is not magic'"
    ));
    test_link_spans!(link_span_shortcut("[drop]", "", "[drop]: drop"));

    test_link_spans!(link_span_reference_unknown("[drop][drop]"));
    test_link_spans!(link_span_shortcut_unknown("[drop]"));
}
