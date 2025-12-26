use std::{borrow::Borrow, collections::HashMap, fmt, hash::Hash};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::{Event, Tag, TagEnd};
use tap::Pipe;
use tracing::{debug, instrument, trace, trace_span};

use mdbookkit::{
    emit_warning,
    markdown::{PatchStream, Spanned},
    plural,
};

use crate::{
    env::EmitConfig,
    link::{ItemLinks, Link, LinkState},
};

mod diagnostic;

#[derive(Debug)]
pub struct Pages<'a, K> {
    pages: HashMap<K, Page<'a>>,
    modified: bool,
}

#[derive(Debug)]
struct Page<'a> {
    source: &'a str,
    links: Vec<Link<'a>>,
}

impl<'a, K: PageKey> Pages<'a, K> {
    #[instrument(level = "debug", "page_read", skip_all)]
    pub fn read<S>(&mut self, key: K, source: &'a str, stream: S) -> Result<()>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        debug!(path = ?key, "reading file");
        self.pages.insert(key, Page::read(source, stream)?);
        Ok(())
    }

    pub fn emit<Q>(&self, key: &Q, options: &EmitConfig) -> Result<String>
    where
        K: Borrow<Q>,
        Q: PageKey + ?Sized,
    {
        self.pages
            .get(key)
            .with_context(|| format!("No such document {key:?}"))
            .inspect_err(emit_warning!())
            .expect("should have document")
            .emit(options)
    }

    pub fn apply<L>(&mut self, links: &HashMap<L, ItemLinks>)
    where
        L: Borrow<str> + Eq + Hash,
    {
        for page in self.pages.values_mut() {
            for link in page.links.iter_mut() {
                if let Some(links) = links.get(link.key()) {
                    *link.state_mut() = LinkState::Resolved(links.clone());
                    self.modified = true;
                }
            }
        }
    }

    pub fn modified(&self) -> bool {
        self.modified
    }
}

impl<'a> Pages<'a, ()> {
    pub fn one<S>(source: &'a str, stream: S) -> Result<Self>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        let mut this = Self::default();
        this.read((), source, stream)?;
        Ok(this)
    }

    pub fn get(&self, options: &EmitConfig) -> Result<String> {
        self.emit(&(), options)
    }
}

impl<'a, K> Default for Pages<'a, K> {
    fn default() -> Self {
        Self {
            pages: Default::default(),
            modified: Default::default(),
        }
    }
}

impl<'a> Page<'a> {
    fn read<S>(source: &'a str, stream: S) -> Result<Self>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        let mut links = Vec::new();
        let mut link: Option<Link<'_>> = None;

        for (event, span) in stream {
            match event {
                Event::End(TagEnd::Link) => match link.take() {
                    Some(open) => {
                        if open.span() == &span {
                            trace!(?span, "link <<<");
                            links.push(open);
                        } else {
                            debug!(?span, "mismatching span, expected {:?}", open.span());
                            bail!("Markdown stream malformed at {span:?}");
                        }
                    }
                    None => {
                        debug!(?span, "unexpected `TagEnd::Link`");
                        bail!("Markdown stream malformed at byte position {span:?}");
                    }
                },
                Event::Start(Tag::Link {
                    dest_url: url,
                    title,
                    ..
                }) => {
                    if link.is_none() {
                        trace!(?span, ?url, ?title, "link >>>");
                        let _span = trace_span!("read_link", ?span, ?url).entered();
                        link = Some(Link::new(span, url, title));
                    } else {
                        debug!(?span, "unexpected `Tag::Link` in `Tag::Link`");
                        bail!("Markdown stream malformed at byte position {span:?}");
                    }
                }
                event => {
                    if let Some(link) = link.as_mut() {
                        trace!(?span, ?event, parent = ?link.span(), "link +++");
                        link.inner_mut().push(event);
                    }
                }
            }
        }

        Ok(Self { source, links })
    }

    fn emit(&self, options: &EmitConfig) -> Result<String> {
        self.links
            .iter()
            .filter_map(|link| link.emit(options))
            .pipe(|stream| PatchStream::new(self.source, stream))
            .into_string()?
            .pipe(Ok)
    }
}

pub trait PageKey: Eq + Hash + fmt::Debug {}

impl<T: Eq + Hash + fmt::Debug> PageKey for T {}

mod iter {
    use std::collections::{
        HashMap,
        hash_map::{Entry, VacantEntry},
    };

    use mdbook_markdown::pulldown_cmark::CowStr;

    use crate::link::{Link, LinkState};

    use super::{Pages, Statistics};

    pub struct PagesIter<T> {
        iter: T,
        stats: Statistics,
    }

    impl<'a, K> Pages<'a, K> {
        pub fn iter(&'_ self) -> PagesIter<impl Iterator<Item = &'_ Link<'a>>> {
            PagesIter {
                iter: self.pages.values().flat_map(|page| page.links.iter()),
                stats: Default::default(),
            }
        }
    }

    impl<'p, 'a: 'p, T: Iterator<Item = &'p Link<'a>>> Iterator for PagesIter<T> {
        type Item = &'p Link<'a>;

        #[inline]
        fn next(&mut self) -> Option<Self::Item> {
            let Statistics {
                links_pending,
                links_resolved,
                ..
            } = &mut self.stats;

            loop {
                let item = self.iter.next()?;
                match item.state() {
                    LinkState::Pending(..) => {
                        *links_pending += 1;
                    }
                    LinkState::Resolved(..) => {
                        *links_resolved += 1;
                    }
                    LinkState::Unparsed => continue,
                }
                return Some(item);
            }
        }
    }

    impl<'p, 'a: 'p, T: Iterator<Item = &'p Link<'a>>> PagesIter<T> {
        #[inline]
        pub fn deduped<F, V>(&mut self, mut f: F) -> HashMap<CowStr<'a>, Option<V>>
        where
            F: FnMut(&'p Link<'a>) -> Option<V>,
        {
            let mut map = Default::default();
            while let Some(link) = self.next() {
                if let Some(entry) = self.record(&mut map, link) {
                    entry.insert(f(link));
                }
            }
            map
        }

        #[inline]
        fn record<'m, V>(
            &mut self,
            map: &'m mut HashMap<CowStr<'a>, V>,
            link: &'p Link<'a>,
        ) -> Option<VacantEntry<'m, CowStr<'a>, V>> {
            let Statistics {
                items_pending,
                items_resolved,
                ..
            } = &mut self.stats;

            let Entry::Vacant(entry) = map.entry(link.key().clone()) else {
                return None;
            };

            match link.state() {
                LinkState::Pending(..) => {
                    *items_pending += 1;
                }
                LinkState::Resolved(..) => {
                    *items_resolved += 1;
                }
                LinkState::Unparsed => {}
            }

            Some(entry)
        }

        pub fn stats(&self) -> &Statistics {
            &self.stats
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Statistics {
    pub links_pending: usize,
    pub items_pending: usize,
    pub links_resolved: usize,
    pub items_resolved: usize,
}

impl Statistics {
    pub fn has_pending(&self) -> bool {
        self.items_pending != 0
    }

    pub fn fmt_pending(&self) -> String {
        let Self {
            links_pending,
            items_pending,
            links_resolved,
            items_resolved,
        } = self;

        let items = match (items_pending, items_resolved) {
            (a, 0) => plural!(a, "item"),
            (a, b) => format!("{a} out of {}", plural!(a + b, "item")),
        };

        let links = match (links_pending, links_resolved) {
            (a, 0) => plural!(a, "link"),
            (a, b) => format!("{a} out of {}", plural!(a + b, "link")),
        };

        format!("{links} containing {items}")
    }

    pub fn fmt_resolved(&self) -> String {
        let Self {
            links_pending,
            items_pending,
            links_resolved,
            items_resolved,
        } = self;

        let links = match (links_pending, links_resolved) {
            (0, b) => plural!(b, "link"),
            (a, b) => format!("{b} out of {}", plural!(a + b, "link")),
        };

        let items = match (items_pending, items_resolved) {
            (0, b) => plural!(b, "item"),
            (a, b) => format!("{b} out of {}", plural!(a + b, "item")),
        };

        format!("{links} containing {items}")
    }
}
