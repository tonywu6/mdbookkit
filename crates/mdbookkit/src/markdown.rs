//! Markdown-related utilities.

use std::{borrow::Cow, ops::Range};

use mdbook_markdown::pulldown_cmark::{Event, Options};
use pulldown_cmark_to_cmark::{Error, State, cmark_resume};

pub fn locate_text(source: &str, sliced: &str) -> Option<Range<usize>> {
    let sliced_lower = sliced.as_ptr();
    let sliced_upper = unsafe { sliced_lower.add(sliced.len()) };
    let source_lower = source.as_ptr();
    let source_upper = unsafe { source_lower.add(source.len()) };
    if source_lower <= sliced_lower && sliced_upper <= source_upper {
        let lower = unsafe { sliced_lower.offset_from_unsigned(source_lower) };
        let upper = unsafe { sliced_upper.offset_from_unsigned(source_lower) };
        Some(lower..upper)
    } else {
        None
    }
}

/// _Patch_ a Markdown string, instead of regenerating it entirely, in order to preserve
/// as much of the original Markdown source as possible, especially with regard to whitespace.
///
/// Currently, when using [`pulldown_cmark_to_cmark`] to generate Markdown from a
/// [`pulldown_cmark::Event`][Event] stream, whitespace is not preserved. This is problematic
/// for mdBook preprocessors, because downstream preprocessors may need to work on
/// syntax that is whitespace-sensitive. Normalizing all whitespace could cause such
/// usage to no longer be recognized.
///
/// `stream` should be an [`Iterator`] that yields tuples of `(events, range)`:
///
/// - `events` should be an [`Iterator`] yielding [`Event`]s which are the replacement
///   Markdown to be rendered into `source` using [`pulldown_cmark_to_cmark`].
///
/// - `range`, when it is `Some(..)`, is the byte span in `source` that should be patched
///   using `events`.
///
///   If it is `None`, `events` are the original [`Event`]s parsed from `source` that are
///   in between two patches. When applying patches that span multiple lines, the original
///   events are necessary for generating Markdown with the correct indentation, such as
///   when the patch occurs within a blockquote.
#[inline]
pub fn patch_stream<'a, E, S>(source: &'a str, stream: S) -> Result<String, Error>
where
    E: Iterator<Item = Event<'a>>,
    S: Iterator<Item = (E, Option<Range<usize>>)>,
{
    let stream = PatchStream {
        stream,
        state: None,
    };
    let mut content = String::with_capacity(source.len());
    let mut emitted = 0..0;
    for chunk in stream {
        let (chunk, span) = chunk?;
        let leading = emitted.end..span.start;
        content.push_str(&source[leading]);
        content.push_str(&chunk);
        emitted = span;
    }
    let trailing = emitted.end..source.len();
    content.push_str(&source[trailing]);
    Ok(content)
}

struct PatchStream<'a, S> {
    stream: S,
    state: Option<State<'a>>,
}

impl<'a, E, S> Iterator for PatchStream<'a, S>
where
    E: Iterator<Item = Event<'a>>,
    S: Iterator<Item = (E, Option<Range<usize>>)>,
{
    type Item = Result<(String, Range<usize>), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (events, span) = self.stream.next()?;
            let state = self.state.take().unwrap_or_default();

            if let Some(span) = span {
                let mut state = state;
                let mut chunk = String::new();
                let mut trailing_newline = false;

                for event in events {
                    trailing_newline = if let Event::InlineHtml(text)
                    | Event::Text(text)
                    | Event::Html(text) = &event
                    {
                        text.ends_with('\n')
                    } else {
                        false
                    };

                    match cmark_resume(std::iter::once(event), &mut chunk, Some(state)) {
                        Ok(next) => state = next,
                        Err(err) => return Some(Err(err)),
                    }
                }

                if trailing_newline
                    && let padding = state.padding.join("")
                    && !padding.is_empty()
                    && chunk.ends_with(&padding)
                {
                    chunk.truncate(chunk.len() - padding.len());
                }

                self.state = Some(state);
                return Some(Ok((chunk, span)));
                //
            } else {
                //
                match cmark_resume(events, &mut NullWriter, Some(state)) {
                    Ok(next) => self.state = Some(next),
                    Err(err) => return Some(Err(err)),
                }

                struct NullWriter;
                impl std::fmt::Write for NullWriter {
                    fn write_str(&mut self, _: &str) -> std::fmt::Result {
                        Ok(())
                    }
                }
            }
        }
    }
}

/// <https://github.com/rust-lang/mdBook/blob/v0.5.1/crates/mdbook-markdown/src/lib.rs#L46-L50>
///
/// See also [`markdown_options`][super::book::BookConfigHelper::markdown_options].
#[inline]
pub const fn default_markdown_options() -> Options {
    Options::empty()
        .union(Options::ENABLE_TABLES)
        .union(Options::ENABLE_FOOTNOTES)
        .union(Options::ENABLE_STRIKETHROUGH)
        .union(Options::ENABLE_TASKLISTS)
        .union(Options::ENABLE_HEADING_ATTRIBUTES)
}

pub type Spanned<T> = (T, Range<usize>);

#[inline(always)]
pub fn replace_char_if_needed<'a, 'r, F>(text: &'a str, mut replacer: F) -> Cow<'a, str>
where
    F: FnMut(char) -> Option<&'r str>,
{
    let mut replaced = Cow::Borrowed(text);

    for (b, c) in text.char_indices() {
        match replaced {
            Cow::Borrowed(text) => match replacer(c) {
                None => {}
                Some(s) => {
                    let mut buf = String::with_capacity(b + s.len());
                    buf.push_str(&text[0..b]);
                    buf.push_str(s);
                    replaced = Cow::Owned(buf);
                }
            },
            Cow::Owned(ref mut buf) => match replacer(c) {
                None => buf.push(c),
                Some(s) => buf.push_str(s),
            },
        }
    }

    replaced
}
