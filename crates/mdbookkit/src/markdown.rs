use std::{borrow::Cow, fmt::Write, ops::Range};

use pulldown_cmark::{Event, Options};
use pulldown_cmark_to_cmark::{cmark, Error};
use tap::Pipe;

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

        let patch = match String::new().pipe(|mut out| cmark(events, &mut out).and(Ok(out))) {
            Err(error) => return Some(Err(error)),
            Ok(patch) => patch,
        };

        self.start = Some(span.end);
        self.patch = Some(patch);

        Some(Ok(Cow::Borrowed(&self.source[start..span.start])))
    }
}

impl<'a, S> PatchStream<'a, S>
where
    Self: Iterator<Item = Result<Cow<'a, str>, Error>>,
{
    pub fn into_string(self) -> Result<String, Error> {
        let mut out = String::new();
        for chunk in self {
            write!(out, "{}", chunk?).unwrap();
        }
        Ok(out)
    }
}

impl<'a, S> PatchStream<'a, S> {
    pub fn new(source: &'a str, stream: S) -> Self {
        Self {
            source,
            stream,
            start: Some(0),
            patch: None,
        }
    }
}

/// <https://github.com/rust-lang/mdBook/blob/v0.4.47/src/utils/mod.rs#L197-L208>
pub fn markdown_options(smart_punctuation: bool) -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    if smart_punctuation {
        opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    }
    opts
}

pub type Spanned<T> = (T, Range<usize>);
