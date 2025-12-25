//! Markdown-related utilities.

use std::{borrow::Cow, fmt::Write, ops::Range};

use mdbook_markdown::pulldown_cmark::{Event, Options};
use pulldown_cmark_to_cmark::{Error, cmark};
use tap::Pipe;
use tracing::{debug, trace, trace_span};

use crate::error::ExpectFmt;

/// _Patch_ a Markdown string, instead of regenerating it entirely, in order to preserve
/// as much of the original Markdown source as possible, especially with regard to whitespace.
///
/// Currently, when using [`pulldown_cmark_to_cmark`] to generate Markdown from a
/// [`pulldown_cmark::Event`][Event] stream, whitespace is not preserved. This is problematic
/// for mdBook preprocessors, because downstream preprocessors may need to work on
/// syntax that is whitespace-sensitive. Normalizing all whitespace could cause such
/// usage to no longer be recognized.
pub struct PatchStream<'a, S> {
    source: &'a str,
    stream: S,
    range: Option<Range<usize>>,
    patch: Option<String>,
}

impl<'a, 'b, E, S> Iterator for PatchStream<'a, S>
where
    E: Iterator<Item = Event<'b>>,
    S: Iterator<Item = Spanned<E>>,
{
    type Item = Result<Cow<'a, str>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let range = self.range.clone()?;

        if let Some(patch) = self.patch.take() {
            trace!("- {range:?} {:?}", &self.source[range.clone()]);
            trace!("+ {range:?} {patch:?}");
            return Some(Ok(Cow::Owned(patch)));
        }

        let Some((events, span)) = self.stream.next() else {
            let range = range.end..;
            trace!("  {range:?}");
            trace!("  EOF");
            self.range = None;
            return Some(Ok(Cow::Borrowed(&self.source[range])));
        };

        if range.start > span.start {
            debug!("span {span:?} is before already yielded span {range:?}");
            return Some(Err(Error::FormatFailed(Default::default())));
        }

        let patch = match trace_span!("chunk", ?span)
            .in_scope(|| String::new().pipe(|mut out| cmark(events, &mut out).and(Ok(out))))
        {
            Err(error) => return Some(Err(error)),
            Ok(patch) => patch,
        };

        self.range = Some(span.clone());
        self.patch = Some(patch);

        let range = range.end..span.start;
        trace!("  {range:?}");
        Some(Ok(Cow::Borrowed(&self.source[range])))
    }
}

impl<'a, S> PatchStream<'a, S> {
    /// Create a new patch stream.
    ///
    /// `stream` should be an [`Iterator`] yielding tuples of `(events, range)`:
    ///
    /// - `events` is an [`Iterator`] yielding [`Event`]s which is the replacement
    ///   Markdown to be rendered into `source` using [`pulldown_cmark_to_cmark`].
    ///
    /// - `range` is a [`Range<usize>`] representing the byte span in `source` that
    ///   should be patched.
    ///
    /// **The yielded ranges must not overlap or decrease**, that is, for `span1` and
    /// `span2`, where `span1` is yielded before `span2`, `span1.end <= span2.start`.
    ///
    /// # Panics
    ///
    /// Panic if ranges in `stream` are not monotonically increasing.
    pub fn new(source: &'a str, stream: S) -> Self {
        Self {
            source,
            stream,
            range: Some(0..0),
            patch: None,
        }
    }
}

impl<'a, S> PatchStream<'a, S>
where
    Self: Iterator<Item = Result<Cow<'a, str>, Error>>,
{
    /// Render the patched Markdown source.
    pub fn into_string(self) -> Result<String, Error> {
        let mut out = String::new();
        for chunk in self {
            write!(out, "{}", chunk?).expect_fmt();
        }
        Ok(out)
    }
}

/// <https://github.com/rust-lang/mdBook/blob/v0.5.1/crates/mdbook-markdown/src/lib.rs#L46-L50>
///
/// See also [`markdown_options`][super::book::BookConfigHelper::markdown_options].
pub const fn default_markdown_options() -> Options {
    Options::empty()
        .union(Options::ENABLE_TABLES)
        .union(Options::ENABLE_FOOTNOTES)
        .union(Options::ENABLE_STRIKETHROUGH)
        .union(Options::ENABLE_TASKLISTS)
        .union(Options::ENABLE_HEADING_ATTRIBUTES)
}

pub type Spanned<T> = (T, Range<usize>);
