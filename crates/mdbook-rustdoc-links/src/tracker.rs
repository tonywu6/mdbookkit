use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::{Debug, Display, Write},
    ops::{ControlFlow, Range},
    path::PathBuf,
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
    config::BaseDir,
    diagnostics::{
        Highlight, IssueLevel, IssueReport, IssueReporter, Note, SourceCode, Suggestion,
        annotate_snippets::AnnotationKind,
    },
    doc_link, emit_debug, emit_trace, emit_warning,
    error::{ExpectFmt, WithDebugContext},
    markdown::{locate_text, patch_stream, replace_char_if_needed},
    plural, try2,
    url::UrlUtil,
    util::{Lexicographic, LexicographicOrd},
    with_bug_report,
};

use crate::{
    builder::{BuildOutput, symlink_dir_all},
    diagnostics::{DiagnosticNotes, RustcDiagnostic, SourceMap, report_level},
    env::Environment,
    markdown::markdown,
};

#[derive(Debug)]
pub struct LinkTracker<'a> {
    links: Vec<Link<'a>>,
    pages: Vec<Page<'a>>,
    notes: DiagnosticNotes,
    symlinks: HashMap<PathBuf, PathBuf>,
    env: Environment,
}

#[derive(Debug)]
struct Page<'a> {
    text: &'a str,
    base: Url,
    link_end: usize,
    trivia: Vec<Vec<Event<'a>>>,
}

#[derive(Debug)]
struct Link<'a> {
    href: Option<Url>,
    kind: LinkType,
    span: SourceSpan,
    elem: Tag<'a>,
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
    dest: Option<Range<usize>>,
}

impl<'a> LinkTracker<'a> {
    pub fn new(env: Environment) -> Self {
        Self {
            links: Default::default(),
            pages: Default::default(),
            notes: Default::default(),
            symlinks: Default::default(),
            env,
        }
    }

    pub fn read(&mut self, text: &'a str, base: Url) -> Result<()> {
        #[allow(clippy::large_enum_variant)]
        enum State<'a> {
            Link(Link<'a>),
            Trivia(Vec<Event<'a>>),
        }

        let mut state = State::Trivia(vec![]);
        let mut trivia = vec![];

        for (event, span) in markdown(text).into_offset_iter() {
            match &mut state {
                State::Trivia(events) => match Link::try_open(text, &event, span) {
                    None => events.push(event),
                    Some(link) => {
                        trivia.push(std::mem::take(events));
                        state = State::Link(link);
                    }
                },

                State::Link(link) => match link.push(event, span)? {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(()) => {
                        let link = match std::mem::replace(&mut state, State::Trivia(vec![])) {
                            State::Link(link) => link,
                            State::Trivia(..) => unreachable!(),
                        };
                        match link.normalized() {
                            ControlFlow::Continue(link) => self.links.push(link),

                            ControlFlow::Break(link) => {
                                let trivia = (trivia.last_mut())
                                    .expect("`trivia` should have at least 1 item");
                                trivia.extend(link.export_original());
                            }
                        }
                    }
                },
            }
        }

        self.pages.push(Page {
            text,
            base,
            link_end: self.links.len(),
            trivia,
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

        lol_html::Settings::new()
            .append_element_content_handler(element!(OUTER_SELECTOR, |_| {
                state.borrow_mut().enter();
                Ok(())
            }))
            .append_element_content_handler(element!("a[href]", |elem| {
                if !state.borrow().has_link() {
                    return Ok(());
                };
                trace!("{elem:?}");

                if let Some(href) = elem.get_attribute("href")
                    && let Some(link) = state.borrow_mut().link()
                    && !eq_escaped(link.dest(), &href)
                {
                    if let Ok(url) = resolve_url(self.env.base_dir(), &output, &href)
                        .with_debug(&*href, "URL")
                        .context("could not convert to a full URL")
                        .or_else(with_bug_report!(emit_warning))
                    {
                        link.href = Some(url)
                    }
                    if let Some(title) = elem.get_attribute("title") {
                        *link.title_mut() = title.into()
                    }
                }

                Ok(())
            }))
            .append_element_content_handler(text!("a[href]", |text| {
                if !state.borrow().has_link() {
                    return Ok(());
                };

                let text = text.as_str();
                if text.is_empty() {
                    let mut state = state.borrow_mut();
                    let text = std::mem::take(&mut state.text_content);
                    #[allow(clippy::unwrap_used)]
                    let link = state.link().unwrap();
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
            }))
            .pipe(|cb| HtmlRewriter::new(cb, |_: &[u8]| ()))
            .pipe(|mut wr| wr.write(output.stdout.as_bytes()).and_then(|_| wr.end()))
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

        for diag in output.stderr {
            if let Ok(line) = locate_diagnostic(&diag)
                .with_context(|| format!("{diag:?}"))
                .context("could not determine primary line")
                .or_else(emit_trace!())
                && let Ok(link) = (self.links.get_mut(line))
                    .with_context(|| format!("{diag:?}"))
                    .with_context(|| format!("line {line}"))
                    .context("line does not belong to any link")
                    .or_else(emit_debug!())
            {
                trace! {
                    "line {}, link {:?}, {}", line + 1, &**link.dest(),
                    diag.rendered.as_deref().unwrap_or(&diag.message)
                };
                link.diagnostics.push(diag);
            }
        }

        if self.env.base_dir().as_http_url().is_none() {
            let dst = self.env.base_dir().as_path();
            let src = &output.metadata.target_directory;

            let (src, dst) = if let Some(target) = output.target.as_deref() {
                (src.join(target).join("doc"), dst.join(target))
            } else {
                (src.join("doc"), dst.to_owned())
            };

            self.symlinks.insert(src.into(), dst);
        }
    }

    pub fn export<'d: 'a>(&'d self) -> ExportedPages<'a> {
        let mut export = ExportedPages::default();

        let iter = self.pages.iter().scan(0usize, |start, page| {
            let links = &self.links[*start..page.link_end];
            *start = page.link_end;
            Some((page, links))
        });

        let mut ctx = IssueReportContext {
            tracker: self,
            notes: self.notes.clone(),
            stats: Default::default(),
        };

        for (page, links) in iter {
            let name = (self.env.page_dir().as_base())
                .show_path(&page.base)
                .to_string();

            let source = SourceCode {
                source_code: page.text,
                source_path: name.into(),
            };

            let issues = links
                .iter()
                .flat_map(|link| ctx.diagnose(link))
                .chain(self.link_summary(links))
                .collect();

            export.issues.push(IssueReporter { issues, source });

            for link in links {
                if let Some(href) = &link.href {
                    export.links.insert(link.dest(), href.as_str());
                }
            }

            let mut trivia = page.trivia.iter();
            let mut links = links.iter();

            let stream = std::iter::from_fn(|| {
                let (trivia, link) = match (trivia.next(), links.next()) {
                    (Some(trivia), Some(link)) => {
                        let link = Some(link.export(&page.base));
                        (Some((Patch::Trivial(trivia.iter().cloned()), None)), link)
                    }
                    (Some(trivia), None) => {
                        (Some((Patch::Trivial(trivia.iter().cloned()), None)), None)
                    }
                    (None, Some(link)) => (None, Some(link.export(&page.base))),
                    (None, None) => return None,
                };
                Some(trivia.into_iter().chain(link))
            })
            .flatten();

            let text = patch_stream(page.text, stream).map_err(<_>::into);

            export.contents.insert(page.base.clone(), text);
        }

        export.stats = ctx.stats;
        export
    }

    pub fn symlink_docs(&self) -> Result<()> {
        let mut symlinks = self.symlinks.iter().collect::<Vec<_>>();

        // ensure that parent dir is linked first, if any
        // for example, build may result in:
        // 1. target/doc => src/api
        // 2. target/aarch64-apple-darwin/doc => src/api/aarch64-apple-darwin
        // then 1. must be linked before 2.
        symlinks.sort_by(|(_, dst1), (_, dst2)| {
            dst1.components().count().cmp(&dst2.components().count())
        });

        for (source, target) in symlinks {
            symlink_dir_all(source, target)
                .context("could not create a symlink as required by the `base-url` option")?;
        }

        Ok(())
    }

    pub fn notes(&mut self) -> &mut DiagnosticNotes {
        &mut self.notes
    }

    pub fn env(&self) -> &Environment {
        &self.env
    }

    fn link_summary(&self, links: &'a [Link<'a>]) -> Option<IssueReport<'a>> {
        let resolved = links
            .iter()
            .filter_map(|link| {
                Highlight::span(link.span.any().clone())
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

#[derive(Default)]
pub struct ExportedPages<'a> {
    pub contents: HashMap<Url, Result<String>>,
    pub issues: Vec<IssueReporter<'a>>,
    pub stats: Statistics,
    pub links: BTreeMap<&'a str, &'a str>,
}

fn resolve_url(base: &BaseDir, output: &BuildOutput<'_>, href: &str) -> Result<Url> {
    if let Ok(href) = href.parse::<Url>() {
        return Ok(href);
    }

    let (name, version, href) = if let Some(href) = href.strip_prefix("../")
        && let Some((lib, _)) = href.split_once('/')
        && let Some(package) = output.crates.get(lib)
    {
        let Package { name, version, .. } = &output.metadata[package];
        (name, version, href)
    } else {
        bail!("unsupported link format")
    };

    let url = (base.as_http_url())
        .unwrap_or_else(|| base.as_file_url())
        .pattern_fill(|group| match group {
            "pkg_name" => Some(name.as_str().into()),
            "version" => Some(version.to_string().into()),
            _ => None,
        });

    let url = if let Some(target) = output.target.as_deref() {
        url.join(target)?
    } else {
        url
    }
    .with_trailing_slash()
    .join(href)?;

    Ok(url)
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

fn eq_escaped(original: &str, encoded: &str) -> bool {
    let decoded = match percent_decode_str(encoded).decode_utf8() {
        Ok(decoded) => decoded,
        Err(..) => return false,
    };
    let decoded = decode_html_entities(&decoded);
    original == decoded
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
    fn try_open(text: &'a str, event: &Event<'a>, span: Range<usize>) -> Option<Self> {
        let Event::Start(
            elem @ Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            },
        ) = event
        else {
            return None;
        };

        if !could_be_item_link(*link_type, dest_url) {
            trace!(?span, dest = ?&**dest_url, "link ...");
            return None;
        }

        trace!(?span, dest = ?&**dest_url, title = ?&**title, "link >>>");
        Some(Link {
            href: None,
            kind: *link_type,
            span: SourceSpan {
                full: span.clone(),
                text: text.len()..0, // empty span
                dest: locate_text(text, dest_url),
            },
            elem: elem.clone(),
            inner_elem: Default::default(),
            normalized: NormalizedLink::borrowed(&text[span]),
            diagnostics: Default::default(),
        })
    }

    #[inline]
    fn dest(&self) -> &CowStr<'a> {
        match self.elem {
            Tag::Link { ref dest_url, .. } => dest_url,
            _ => unreachable!(),
        }
    }

    fn title(&self) -> &CowStr<'a> {
        match self.elem {
            Tag::Link { ref title, .. } => title,
            _ => unreachable!(),
        }
    }

    fn title_mut(&mut self) -> &mut CowStr<'a> {
        match self.elem {
            Tag::Link { ref mut title, .. } => title,
            _ => unreachable!(),
        }
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

    #[instrument(level = "debug", skip_all, fields(link = ?&**self.dest(), span = ?self.span.full))]
    fn normalized(mut self) -> ControlFlow<Self, Self> {
        let Self {
            kind,
            inner_elem,
            normalized,
            ..
        } = &self;

        let is_shortcut = if matches!(kind, CollapsedUnknown | ShortcutUnknown)
            && inner_elem.len() == 1
            && let Some(Event::Text(text) | Event::Code(text)) = inner_elem.first()
            && text == self.dest()
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
            return ControlFlow::Continue(self);
        }

        // https://spec.commonmark.org/0.31.2/#link-title
        // > link titles may span multiple lines
        let title = replace_char_if_needed(self.title(), |c| match c {
            '\r' => Some("&#13;"),
            '\n' => Some("&#10;"),
            _ => None,
        })
        .into();

        let link = match kind {
            ReferenceUnknown | CollapsedUnknown | ShortcutUnknown => Tag::Link {
                link_type: Reference,
                dest_url: "".into(),
                title,
                id: self.dest().clone(),
            },
            link_class!(href_defined) => Tag::Link {
                link_type: Inline,
                dest_url: self.dest().clone(),
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

        let Ok((text, span)) = try2!({
            let mut text = String::with_capacity(normalized.as_ref().len());

            // dropping the state because we are not
            // appending the final link definition
            let _ = cmark_resume(events, &mut text, None)?;

            let span = {
                let mut state = None;
                for (event, span) in markdown(&text).into_offset_iter() {
                    match state.as_mut() {
                        None => {
                            state = Link::try_open(&text, &event, span);
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
        })
        .context("internal error while parsing link")
        .or_else(emit_debug!()) else {
            return ControlFlow::Break(self);
        };

        // link text may still contain newlines
        let text = text.replace('\n', " ");
        // this is acceptable even within `inline code`:
        // https://spec.commonmark.org/0.31.2/#code-spans

        self.normalized = NormalizedLink::Normalized { text, span };

        ControlFlow::Continue(self)
    }

    fn export<T>(
        &'a self,
        base: &Url,
    ) -> (
        Patch<T, impl Iterator<Item = Event<'a>>, impl Iterator<Item = Event<'a>>>,
        Option<Range<usize>>,
    ) {
        match self.export_modified(base) {
            Some(link) => (Patch::Updated(link), Some(self.span.full.clone())),
            None => (Patch::Skipped(self.export_original()), None),
        }
    }

    fn export_modified(&'a self, base: &Url) -> Option<impl Iterator<Item = Event<'a>>> {
        let Self {
            href, inner_elem, ..
        } = self;

        let href = href.as_ref()?;
        let href = if let Some(href) = base.as_base().make_relative(href) {
            href.consume_with(CowStr::from)
        } else {
            CowStr::Borrowed(href.as_str())
        };

        let iter = std::iter::once(Event::Start(Tag::Link {
            link_type: Inline,
            dest_url: href,
            title: self.title().clone(),
            id: CowStr::Borrowed(""),
        }))
        .chain(inner_elem.iter().cloned())
        .chain(std::iter::once(Event::End(TagEnd::Link)));

        Some(iter)
    }

    fn export_original(&self) -> impl Iterator<Item = Event<'a>> {
        std::iter::once(Event::Start(self.elem.clone()))
            .chain(self.inner_elem.iter().cloned())
            .chain(std::iter::once(Event::End(TagEnd::Link)))
    }
}

enum Patch<T, S, L> {
    Trivial(T),
    Skipped(S),
    Updated(L),
}

impl<'a, T, S, L> Iterator for Patch<T, S, L>
where
    T: Iterator<Item = Event<'a>>,
    S: Iterator<Item = Event<'a>>,
    L: Iterator<Item = Event<'a>>,
{
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Trivial(iter) => iter.next(),
            Self::Skipped(iter) => iter.next(),
            Self::Updated(iter) => iter.next(),
        }
    }
}

impl SourceSpan {
    fn any(&self) -> &Range<usize> {
        self.dest.as_ref().unwrap_or(&self.full)
    }
}

struct IssueReportContext<'a> {
    tracker: &'a LinkTracker<'a>,
    notes: DiagnosticNotes,
    stats: Statistics,
}

impl<'a> IssueReportContext<'a> {
    fn diagnose(&mut self, link: &'a Link<'a>) -> Vec<IssueReport<'a>> {
        let mut issues = Vec::with_capacity(link.diagnostics.len());
        let mut seen = BTreeSet::new();

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

        let resolved = link.href.is_some();
        let rustdoc_warnings = !issues.is_empty();
        let likely_intra_doc = !matches!(link.kind, link_class!(href_defined));

        if resolved {
            self.stats.resolved += 1;
        } else if likely_intra_doc || rustdoc_warnings {
            self.stats.unresolved += 1;
        }

        if resolved && rustdoc_warnings {
            self.stats.has_warnings += 1;
        }

        if likely_intra_doc && !resolved && !rustdoc_warnings {
            let issue = IssueReport::level(IssueLevel::Warning)
                .title("unresolved link")
                .annotations(vec![
                    Highlight::span(link.span.full.clone())
                        .kind(AnnotationKind::Primary)
                        .label("rustdoc did not process this link")
                        .build(),
                ])
                .build();
            issues.push(issue);
        }

        issues
    }

    fn augment_unresolved(&mut self, link: &Link<'_>, report: &mut IssueReport<'_>) {
        let could_be_a_path = matches!(link.kind, link_class!(href_defined))
            && !link.dest().contains("::")
            && !link.dest().contains("<");

        if could_be_a_path {
            let help = {
                "if this is meant to be a path to another file, you may prepend \
                the path with `./`\nwhich will silence this warning"
            };
            let suggestion = IssueReport::level(IssueLevel::Help)
                .title(help)
                .patches(if let Some(span) = &link.span.dest {
                    let span = span.start..span.start;
                    vec![Suggestion::span(span).repl("./").build()]
                } else {
                    vec![]
                })
                .build();
            report.secondary(suggestion);
        }

        let could_be_top_level = report.iter_labels().any(|label| {
            label.ends_with(" in scope") || label.contains(" in module `temporary_crate_")
        });

        if could_be_top_level && let Some(note) = self.notes.note_options_specified() {
            report.note(Note::note(note));
        }

        let specifies_crate = if link.dest().starts_with("crate::") {
            Some("crate")
        } else if link.dest().starts_with("self::") {
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
                let help3 = doc_link!(see = "faq#no-item--in-module-temporary_crate_0");
                report
                    .note(Note::help(help1))
                    .note(Note::help(help2))
                    .note(Note::help(help3));
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
                } else if let Some(source) = &link.span.dest
                    && let Some(mapped) = &span.dest
                    && mapped.start <= lower
                    && lower <= mapped.end
                {
                    (source, (source.start + lower - mapped.start))
                } else {
                    return Some(link.span.full.clone());
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
    resolved: usize,
    unresolved: usize,
    has_warnings: usize,
}

impl Display for Statistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            resolved,
            unresolved,
            has_warnings,
        } = self;
        let processed = resolved + unresolved;
        write! { f,
            "processed {processed}: {resolved} resolved",
            processed = plural!(processed, "link"),
        }?;
        if *unresolved > 0 {
            write!(f, "; {unresolved} unresolved")?
        }
        if *has_warnings > 0 {
            write! { f, "; {}",plural!(has_warnings, "has warnings", "have warnings") }?
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use mdbookkit::diagnostics::{
        Highlight, IssueLevel, IssueReport, SourceCode,
        annotate_snippets::{AnnotationKind, Renderer, renderer::DecorStyle},
        issue_to_report,
    };
    use mdbookkit_testing::{
        AssertUtil, default_assert,
        snapbox::{Data, data::DataFormat, utils::current_dir},
    };

    use crate::env::Environment;

    use super::{LinkTracker, SourceSpan};

    fn print_link_spans(span: SourceSpan) -> IssueReport<'static> {
        let SourceSpan { full, text, dest } = span;
        IssueReport::level(IssueLevel::Warning)
            .title("link")
            .annotations(vec![
                Highlight::span(full.clone())
                    .kind(AnnotationKind::Primary)
                    .build(),
            ])
            .secondary(vec![
                IssueReport::level(IssueLevel::Warning)
                    .title("link dest")
                    .annotations(if let Some(dest) = dest {
                        vec![Highlight::span(dest).kind(AnnotationKind::Primary).build()]
                    } else {
                        vec![Highlight::span(full).kind(AnnotationKind::Visible).build()]
                    })
                    .build(),
                IssueReport::level(IssueLevel::Warning)
                    .title("link text")
                    .annotations(vec![
                        Highlight::span(text).kind(AnnotationKind::Primary).build(),
                    ])
                    .build(),
            ])
            .build()
    }

    fn test_link_spans(text: &str, expected: Data) -> Result<()> {
        let mut tracker = LinkTracker::new(Environment::default());
        let root = tracker.env.page_dir();
        let path = root.join("index.md").unwrap();
        tracker.read(text, path)?;
        let span = tracker.links[0].span.clone();
        let source = SourceCode {
            source_code: text,
            source_path: "<anon>".into(),
        };
        let report = print_link_spans(span);
        let report = issue_to_report(report, source);
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

    test_link_spans!(link_span_shortcut_with_inline_mapped("[*PhantomData*]"));
    test_link_spans!(link_span_shortcut_with_inline_unmapped(
        "[PhantomData<fn()>]"
    ));
}
