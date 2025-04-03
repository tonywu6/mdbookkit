//! Markdown-related utilities.

use std::{borrow::Cow, fmt::Write, ops::Range};

use pulldown_cmark::{Event, Options};
use pulldown_cmark_to_cmark::{cmark, Error};
use tap::Pipe;

/// _Patch_ a Markdown string, instead of regenerating it entirely.
///
/// Currently, whitespace is NOT preserved when using [`pulldown_cmark_to_cmark`] to
/// generate Markdown from a [`pulldown_cmark::Event`] stream.
///
/// This is problematic for mdBook preprocessors, because preprocessors downstream
/// may need to work on syntax that is whitespace-sensitive. Normalizing all whitespace
/// could cause such usage to no longer be recognized. An example is [`mdbook-alerts`][alerts]
/// which works on GitHub's ["alerts"][gh-alerts] syntax.
///
/// [alerts]: https://crates.io/crates/mdbook-alerts
/// [gh-alerts]: https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#alerts
pub struct PatchStream<'a, S> {
    source: &'a str,
    stream: S,
    start: Option<usize>,
    patch: Option<String>,
}

impl<'a, 'b, E, S> Iterator for PatchStream<'a, S>
where
    E: Iterator<Item = Event<'b>>,
    S: Iterator<Item = Spanned<E>>,
{
    type Item = Result<Cow<'a, str>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.start?;

        if let Some(patch) = self.patch.take() {
            return Some(Ok(Cow::Owned(patch)));
        }

        let Some((events, span)) = self.stream.next() else {
            self.start = None;
            return Some(Ok(Cow::Borrowed(&self.source[start..])));
        };

        if start > span.start {
            panic!("span {span:?} is backwards from already yielded span ending at {start}")
        }

        let patch = match String::new().pipe(|mut out| cmark(events, &mut out).and(Ok(out))) {
            Err(error) => return Some(Err(error)),
            Ok(patch) => patch,
        };

        self.start = Some(span.end);
        self.patch = Some(patch);

        Some(Ok(Cow::Borrowed(&self.source[start..span.start])))
    }
}

impl<'a, S> PatchStream<'a, S> {
    /// Create a new patch stream.
    ///
    /// `stream` should be an [`Iterator`] yielding tuples of (`events`, `range`):
    ///
    /// - `events` is an [`Iterator`] yielding [`Event`]s which is the replacement
    ///    Markdown to be rendered into `source` using [`pulldown_cmark_to_cmark`].
    ///
    /// - `range` is a [`Range<usize>`] representing the byte span in `source` that
    ///   should be patched.
    ///
    /// **The yielded ranges must not overlap or decrease**, that is, for `span1` and
    /// `span2`, where `span1` is yielded before `span2`, `span1.end <= span2.start`.
    ///
    /// ## Panics
    ///
    /// Panic if ranges in `stream` are not monotonically increasing.
    pub fn new(source: &'a str, stream: S) -> Self {
        Self {
            source,
            stream,
            start: Some(0),
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
            write!(out, "{}", chunk?).unwrap();
        }
        Ok(out)
    }
}

/// <https://github.com/rust-lang/mdBook/blob/v0.4.47/src/utils/mod.rs#L197-L208>
pub fn mdbook_markdown() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts
}

pub type Spanned<T> = (T, Range<usize>);
