use std::{borrow::Borrow, collections::HashMap, fmt::Debug, hash::Hash};

use anyhow::{bail, Context, Result};
use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use tap::Pipe;

use crate::{
    env::EmitConfig,
    link::{ItemLinks, Link, LinkState},
    markdown::PatchStream,
    Item, Spanned,
};

#[derive(Debug, Default)]
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

    pub fn emit<Q>(&self, key: &Q, options: &EmitConfig) -> Result<PatchStream<'a>>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + Debug + ?Sized,
    {
        let page = self.pages.get(key);
        let page = page.with_context(|| format!("no such document {key:?}"))?;
        page.emit(options)
    }

    pub fn items(&self) -> HashMap<CowStr<'a>, &Item> {
        self.pages
            .values()
            .flat_map(|page| page.links.iter())
            .filter_map(|link| link.item().map(|item| (link.url.clone(), item)))
            .collect::<HashMap<_, _>>()
    }

    pub fn links(&self) -> HashMap<CowStr<'a>, ItemLinks> {
        self.pages
            .values()
            .flat_map(|page| page.links.iter())
            .filter_map(|link| link.link().map(|item| (link.url.clone(), item)))
            .collect::<HashMap<_, _>>()
    }

    pub fn apply<L>(&mut self, links: &HashMap<L, ItemLinks>)
    where
        L: Borrow<str> + Eq + Hash,
    {
        for page in self.pages.values_mut() {
            for link in page.links.iter_mut() {
                if let Some(links) = links.get(&link.url) {
                    link.state = LinkState::Resolved(links.clone());
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

    pub fn get(&self, options: &EmitConfig) -> Result<PatchStream<'a>> {
        self.emit(&(), options)
    }
}

impl<'a> Page<'a> {
    fn read<S>(source: &'a str, stream: S) -> Result<Self>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        let mut links = Vec::new();
        let mut link: Option<Link> = None;

        for (event, span) in stream {
            if matches!(event, Event::End(TagEnd::Link)) {
                match link.take() {
                    Some(link) => {
                        if link.span == span {
                            links.push(link);
                            continue;
                        } else {
                            bail!("mismatching span, expected {:?} != {span:?}", link.span)
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
                    link.inner.push(event);
                }
                continue;
            };

            if link.is_some() {
                bail!("unexpected `Tag::Link` in `Tag::Link`")
            }

            link = Some(Link::new(span, url, title));
        }

        Ok(Self { source, links })
    }

    fn emit(&self, options: &EmitConfig) -> Result<PatchStream<'a>> {
        self.links
            .iter()
            .filter_map(|link| {
                Tag::Link {
                    dest_url: link.emit(options)?.to_string().into(),
                    link_type: LinkType::Inline,
                    title: link.title.clone(),
                    id: CowStr::Borrowed(""),
                }
                .pipe(|tag| std::iter::once(Event::Start(tag)))
                .chain(link.inner.iter().cloned())
                .chain(std::iter::once(Event::End(TagEnd::Link)))
                .pipe(|events| Some((events, link.span.clone())))
            })
            .pipe(|stream| PatchStream::patch(self.source, stream))
    }
}
