use std::{
    borrow::Borrow,
    collections::HashMap,
    fmt::{self, Debug},
    hash::Hash,
};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::{CowStr, Event, Tag, TagEnd};
use tap::Pipe;

use mdbookkit::markdown::{PatchStream, Spanned};

use crate::{
    env::EmitConfig,
    item::Item,
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

impl<'a, K: Eq + Hash> Pages<'a, K> {
    pub fn read<S>(&mut self, key: K, source: &'a str, stream: S) -> Result<()>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        self.pages.insert(key, Page::read(source, stream)?);
        Ok(())
    }

    pub fn emit<Q>(&self, key: &Q, options: &EmitConfig) -> Result<String>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + fmt::Debug + ?Sized,
    {
        let page = self.pages.get(key);
        let page = page.with_context(|| format!("no such document {key:?}"))?;
        page.emit(options)
    }

    pub fn items(&self) -> HashMap<CowStr<'a>, &Item> {
        self.pages
            .values()
            .flat_map(|page| page.links.iter())
            .filter_map(|link| link.item().map(|item| (link.key().clone(), item)))
            .collect::<HashMap<_, _>>()
    }

    pub fn links(&self) -> HashMap<CowStr<'a>, ItemLinks> {
        self.pages
            .values()
            .flat_map(|page| page.links.iter())
            .filter_map(|link| link.link().map(|item| (link.key().clone(), item)))
            .collect::<HashMap<_, _>>()
    }

    pub fn apply<L>(&mut self, links: &HashMap<L, ItemLinks>)
    where
        L: Borrow<str> + Eq + Hash,
    {
        for page in self.pages.values_mut() {
            for link in page.links.iter_mut() {
                if let Some(links) = links.get(link.key()) {
                    *link.state() = LinkState::Resolved(links.clone());
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
            if matches!(event, Event::End(TagEnd::Link)) {
                match link.take() {
                    Some(link) => {
                        if link.span() == &span {
                            links.push(link);
                            continue;
                        } else {
                            bail!("mismatching span, expected {:?}, got {span:?}", link.span())
                        }
                    }
                    None => bail!("unexpected `TagEnd::Link` at {span:?}"),
                }
            }

            let Event::Start(Tag::Link {
                dest_url: url,
                title,
                ..
            }) = event
            else {
                if let Some(link) = link.as_mut() {
                    link.inner().push(event);
                }
                continue;
            };

            if link.is_some() {
                bail!("unexpected `Tag::Link` in `Tag::Link` at {span:?}")
            }

            link = Some(Link::new(span, url, title));
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
