use std::{
    cell::Cell,
    collections::HashSet,
    fmt::{Debug, Display, Write},
    hash::Hash,
    ops::{ControlFlow, Range},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{
    Package,
    diagnostic::{Diagnostic, DiagnosticSpan},
};
use html_escape::decode_html_entities;
use lol_html::{HtmlRewriter, Settings, element};
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
    diagnostics::{
        Highlight, IssueLevel, IssueReport, Note, Suggestion, annotate_snippets::AnnotationKind,
    },
    emit,
    error::{Break, ConsumeError, ExpectFmt},
    markdown::PatchStream,
    plural,
};

use crate::{
    builder::BuildOutput,
    diagnostics::{RustcDiagnostic, SourceMap},
    markdown::markdown,
};

#[derive(Debug, Default)]
pub struct LinkTracker<'a> {
    links: Vec<Link<'a>>,
    pages: Vec<Page<'a>>,
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

#[derive(Debug)]
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

    pub fn rustdoc_output(&mut self, output: BuildOutput<'_>) {
        let BuildOutput {
            ref stdout,
            ref stderr,
            ..
        } = output;

        let row = Cell::<Option<usize>>::new(None);

        Settings {
            element_content_handlers: vec![
                element!(OUTER_SELECTOR, |_| {
                    row.update(|i| match i {
                        Some(i) => Some(i + 1),
                        None => Some(0),
                    });
                    Ok(())
                }),
                element!("a[href]", |elem| {
                    if let Some(row) = row.get()
                        && let Some(href) = elem.get_attribute("href")
                        && let link = &mut self.links[row]
                        && !eq_escaped(&link.dest, &href)
                    {
                        if let Ok(url) = resolve_url(&output, &href)
                            .with_context(|| format!("Failed to convert link: {href}"))
                            .or_warn(emit!())
                        {
                            link.href = Some(url)
                        }
                        if let Some(title) = elem.get_attribute("title") {
                            link.title = title.into()
                        }
                    }
                    Ok(())
                }),
            ],
            ..Default::default()
        }
        .pipe(|cb| HtmlRewriter::new(cb, |_: &[u8]| ()))
        .pipe(|mut wr| wr.write(stdout.as_bytes()).and_then(|_| wr.end()))
        .context("unexpected error from HtmlRewriter")
        .or_debug(emit!())
        .ok();

        for line in String::from_utf8_lossy(stderr).lines() {
            if let Ok(diag) = serde_json::from_str::<Diagnostic>(line)
                && let Some(line) = locate_diagnostic(&diag)
                && let Some(link) = self.links.get_mut(line)
            {
                link.diagnostics.push(diag);
            }
        }
    }

    pub fn export<'d: 'a>(&'d self) -> ExportedPages<'d> {
        let mut contents = Vec::with_capacity(self.pages.len());
        let mut issues = Vec::with_capacity(self.pages.len());

        let links = self.pages.iter().scan(0usize, |start, page| {
            let links = &self.links[*start..page.link_end];
            *start = page.link_end;
            Some((page, links))
        });

        let mut stats = Statistics::default();

        for (page, links) in links {
            let page_issues = links
                .iter()
                .inspect(|link| stats.count(link))
                .flat_map(|link| link.diagnose(self))
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

fn could_be_item_link(kind: LinkType, url: &str) -> bool {
    if matches!(kind, Autolink | Email | WikiLink { .. }) {
        return false;
    }

    let url = if let Some(idx) = url.rfind('#') {
        &url[..idx]
    } else {
        url
    };

    if url.is_empty() {
        return false;
    }

    if !(url.chars()).all(|c| c.is_alphanumeric() || ":_<>, !*&;@()'".contains(c)) {
        return false;
    }

    if matches!(kind, ShortcutUnknown)
        && let Some(suffix) = url.strip_prefix('!')
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
            Some(span.line_start - 1)
        } else {
            None
        }
    })
}

macro_rules! data_attr {
    () => {
        env!("CARGO_PKG_NAME")
    };
}
static OUTER_SELECTOR: &str = concat!("span[", data_attr!(), "]");
static COMMENT_PREFIX: &str = concat!("//! - <span ", data_attr!(), ">");
static COMMENT_SUFFIX: &str = "</span>";

macro_rules! has_link_dest {
    () => {
        Inline | Reference | Collapsed | Shortcut
    };
}

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
            trace!(?span, ?dest_url, "link ...");
            return None;
        }

        trace!(?span, ?dest_url, "link >>>");
        Some(Link {
            href: None,
            kind: link_type,
            span: SourceSpan {
                full: span.clone(),
                text: text.len()..0,
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
                bail!("Markdown stream malformed at {span:?}");
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
                    bail!("Markdown stream malformed at {span:?}");
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
            Autolink | Email | WikiLink { .. } => unreachable!(),
        } && !normalized.as_ref().contains('\n');

        if is_one_line {
            return Ok(self);
        }

        let link = match kind {
            has_link_dest!() => Tag::Link {
                link_type: Inline,
                dest_url: dest.clone(),
                title: "".into(),
                id: "".into(),
            },
            ReferenceUnknown | CollapsedUnknown | ShortcutUnknown => Tag::Link {
                link_type: Reference,
                dest_url: "".into(),
                title: "".into(),
                id: dest.clone(),
            },
            Autolink | Email | WikiLink { .. } => {
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
        .context("Internal error while parsing link; the link will be skipped")
        .or_warn(emit!())?;

        self.normalized = NormalizedLink::Normalized { text, span };

        Ok(self)
    }

    fn diagnose(&'a self, tracker: &LinkTracker<'a>) -> Vec<IssueReport<'a>> {
        let mut issues = Vec::with_capacity(self.diagnostics.len());
        let mut seen = HashSet::new();

        if self.href.is_none() && self.diagnostics.is_empty() {
            let span = &self.span.dest;

            let issue = IssueReport::level(IssueLevel::Warning)
                .title("unresolved link")
                .annotations(vec![
                    Highlight::span(span.clone())
                        .kind(AnnotationKind::Primary)
                        .label("rustdoc did not process this link")
                        .build(),
                ])
                .notes(vec![
                    Note::level(IssueLevel::Note)
                        .message("rustdoc may not support the syntax of this item")
                        .build(),
                ])
                .secondary(if matches!(self.kind, has_link_dest!()) {
                    vec![suggest_path_prefix(span.clone())]
                } else {
                    vec![]
                })
                .build();

            issues.push(issue);
        }

        for diagnostic in self.diagnostics.iter() {
            if seen.replace(DiagnosticKey(diagnostic)).is_some() {
                continue;
            }

            let mut issue = RustcDiagnostic {
                diagnostic,
                source_map: tracker,
            }
            .conv::<IssueReport>();

            if has_error_code(diagnostic, "rustdoc::broken_intra_doc_links") {
                if matches!(self.kind, has_link_dest!())
                    && diagnostic.message.starts_with("unresolved link to")
                {
                    issue.secondary(suggest_path_prefix(self.span.dest.clone()));
                }
                if let Some(prefix) = {
                    if self.dest.starts_with("crate::") {
                        Some("crate::")
                    } else if self.dest.starts_with("self::") {
                        Some("self::")
                    } else {
                        None
                    }
                } {
                    let help = format! {
                        "the `{prefix}...` usage is not supported with this preprocessor\n\
                        specify the crate name, or use the `build.preludes` option to \
                        introduce this item into scope"
                    };
                    issue.note(Note::level(IssueLevel::Note).message(help).build());
                }
            }

            issues.push(issue);
        }

        issues
    }

    fn export(&'a self) -> Option<impl Iterator<Item = Event<'a>>> {
        let Self {
            href,
            title,
            inner_elem,
            kind,
            dest,
            ..
        } = self;

        let href = href.as_ref()?;

        let iter = std::iter::once(Event::Start(Tag::Link {
            link_type: Inline,
            dest_url: CowStr::Borrowed(href.as_str()),
            title: title.clone(),
            id: CowStr::Borrowed(""),
        }))
        .chain(inner_elem.iter().map(|elem| match elem {
            Event::Text(text) | Event::Code(text) => {
                if matches!(kind, CollapsedUnknown | ShortcutUnknown)
                    && **text == **dest
                    && let Some((_, text)) = text.split_once('@')
                {
                    match elem {
                        Event::Text(..) => Event::Text(text.into()),
                        Event::Code(..) => Event::Code(text.into()),
                        _ => unreachable!(),
                    }
                } else {
                    elem.clone()
                }
            }
            elem => elem.clone(),
        }))
        .chain(std::iter::once(Event::End(TagEnd::Link)));

        Some(iter)
    }
}

fn has_error_code(diag: &Diagnostic, code: &str) -> bool {
    diag.code.as_ref().map(|c| c.code == code).unwrap_or(false)
}

fn suggest_path_prefix<'a>(span: Range<usize>) -> IssueReport<'a> {
    let help = {
        "to indicate that this is a relative path (which will silence this warning),\n\
        prepend the link with `./`"
    };
    let span = span.start..span.start;
    IssueReport::level(IssueLevel::Help)
        .title(help)
        .patches(vec![Suggestion::span(span).repl("./").build()])
        .build()
}

impl SourceMap for LinkTracker<'_> {
    fn map_span(&self, span: &DiagnosticSpan) -> Option<Range<usize>> {
        if span.line_start != span.line_end {
            return None;
        }

        let link = self.links.get(span.line_start - 1)?;
        let line = span.text.first()?;

        let lower = line.highlight_start - 1;
        let upper = line.highlight_end - 1;

        let lower = line.text.char_indices().nth(lower)?.0;
        let upper = line.text.char_indices().nth(upper)?.0;

        let lower = lower.checked_sub(COMMENT_PREFIX.len())?;
        let upper = upper.checked_sub(COMMENT_PREFIX.len())?;
        let len = upper.checked_sub(lower)?;

        let lower = match &link.normalized {
            NormalizedLink::Unmodified { .. } => link.span.full.start + lower,
            NormalizedLink::Normalized { span, .. } => {
                if span.text.start <= lower && span.text.end >= upper {
                    link.span.text.start + lower - span.text.start
                } else if span.dest.start <= lower && span.dest.end >= upper {
                    link.span.dest.start + lower - span.dest.start
                } else {
                    link.span.full.start + lower - span.full.start
                }
            }
        };

        let upper = lower + len;
        Some(lower..upper)
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

impl PartialEq for DiagnosticKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
            && self.0.spans.len() == other.0.spans.len()
            && self.spans().zip(other.spans()).all(|(s1, s2)| s1 == s2)
    }
}

impl DiagnosticKey<'_> {
    fn key(&self) -> impl Eq + Hash {
        (&self.0.level, &self.0.code, &self.0.message)
    }

    fn spans(&self) -> impl Iterator<Item = impl Eq + Hash> {
        (self.0.spans.iter()).map(|span| {
            (
                &span.label,
                &span.line_start,
                &span.column_start,
                &span.line_end,
                &span.column_end,
                &span.file_name,
                &span.is_primary,
                &span.suggested_replacement,
            )
        })
    }
}

impl Eq for DiagnosticKey<'_> {}

impl Hash for DiagnosticKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
        for span in self.spans() {
            span.hash(state);
        }
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
            "Processed {processed}: \
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
