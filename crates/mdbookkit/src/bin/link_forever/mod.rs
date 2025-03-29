use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    ops::Range,
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use mdbook::utils::unique_id_from_content;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use tap::{Pipe, Tap, TapFallible};
use url::Url;

use crate::{
    log_debug,
    markdown::{PatchStream, Spanned},
};

#[cfg(feature = "common-logger")]
mod diagnostic;
pub mod env;

use self::env::Environment;

pub trait PermalinkFormat {
    fn link_to(&self, relpath: &str) -> Result<Url, url::ParseError>;
}

impl Environment {
    pub fn resolve(&self, content: &mut Pages<'_>) {
        self.validate_self();

        let fragments = content.take_fragments();

        for (base, link) in content
            .pages
            .iter_mut()
            .flat_map(|(base, page)| page.rel_links.iter_mut().map(move |link| (base, link)))
        {
            let Ok(mut url) = base
                .join(&link.link)
                .context("couldn't derive url")
                .tap_err(log_debug!())
            else {
                link.status = LinkStatus::Ignored;
                continue;
            };

            if url.scheme() != "file" {
                link.status = LinkStatus::Ignored;
                continue;
            };

            let Ok(path) = url.to_file_path() else {
                link.status = LinkStatus::Ignored;
                continue;
            };

            if !matches!(
                path.try_exists()
                    .context("could not access path")
                    .tap_err(log_debug!()),
                Ok(true)
            ) {
                link.status = LinkStatus::NoSuchPath;
                continue;
            }

            let Ok(rel) = self
                .book_src
                .make_relative(&url)
                .context("url is from a different origin")
                .tap_err(log_debug!())
            else {
                continue;
            };

            if !rel.starts_with("../")
                && !self
                    .config
                    .always_link
                    .iter()
                    .any(|suffix| url.path().ends_with(suffix))
            {
                link.status = LinkStatus::Published;

                let Some(fragment) = url
                    .fragment()
                    .and_then(|f| percent_decode_str(f).decode_utf8().ok().or(Some(f.into())))
                    .map(|f| f.into_owned())
                else {
                    continue;
                };

                url.set_fragment(None);

                let found = fragments
                    .0
                    .get(&url)
                    .map(|f| f.contains(&fragment))
                    .unwrap_or(false);

                url.set_fragment(Some(&fragment));

                if !found {
                    link.status = LinkStatus::NoSuchFragment;
                }

                continue;
            }

            let Ok(rel) = self
                .vcs_root
                .make_relative(&url)
                .context("url is from a different origin")
                .tap_err(log_debug!())
            else {
                continue;
            };

            if rel.starts_with("../") {
                link.status = LinkStatus::External;
                continue;
            }

            match self.fmt_link.link_to(&rel) {
                Ok(href) => {
                    link.status = LinkStatus::Permalink;
                    link.link = href.as_str().to_owned().into();
                }
                Err(err) => link.status = LinkStatus::ParseError(err),
            }
        }

        fragments.tap_mut(|f| f.restore(content));
    }

    #[inline]
    fn validate_self(&self) {
        debug_assert!(
            self.book_src.as_str().ends_with('/'),
            "book_src should have a trailing slash, got {}",
            self.book_src
        );

        debug_assert!(
            self.vcs_root.as_str().ends_with('/'),
            "vcs_root should have a trailing slash, got {}",
            self.vcs_root
        );
    }
}

pub struct Pages<'a> {
    pages: HashMap<Arc<Url>, Page<'a>>,
    markdown: Options,
}

struct Page<'a> {
    source: &'a str,
    rel_links: Vec<RelativeLink<'a>>,
    fragments: HashSet<String>,
}

struct RelativeLink<'a> {
    span: Range<usize>,
    link: CowStr<'a>,
    text: Vec<Event<'a>>,
    title: CowStr<'a>,
    status: LinkStatus,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum LinkStatus {
    /// Not a file: URL
    #[default]
    Ignored,
    /// Link to a file under src/
    Published,
    /// Link to a file under source control
    Permalink,
    /// Link to a file outside source control
    External,
    /// Link to a file that cannot be accessed
    NoSuchPath,
    /// Link to a fragment that does not exist in a page
    NoSuchFragment,
    /// Link to a file under source control but URL parsing failed
    ParseError(url::ParseError),
}

impl<'a> Pages<'a> {
    pub fn new(markdown: Options) -> Self {
        Self {
            pages: Default::default(),
            markdown,
        }
    }

    pub fn insert(&mut self, url: Url, source: &'a str) -> Result<&mut Self> {
        let stream = Parser::new_ext(source, self.markdown).into_offset_iter();
        let page = Page::read(source, stream)?;
        self.pages.insert(url.into(), page);
        Ok(self)
    }

    pub fn emit<Q>(&self, key: &Q) -> Result<String>
    where
        Arc<Url>: Borrow<Q>,
        Q: Eq + Hash + Debug + ?Sized,
    {
        let page = self.pages.get(key);
        let page = page.with_context(|| format!("no such document {key:?}"))?;
        page.emit()
    }

    #[must_use]
    fn take_fragments(&mut self) -> Fragments {
        self.pages
            .iter_mut()
            .map(|(url, page)| (url.clone(), std::mem::take(&mut page.fragments)))
            .collect::<HashMap<_, _>>()
            .pipe(Fragments)
    }
}

impl<'a> Page<'a> {
    fn read<S>(source: &'a str, stream: S) -> Result<Self>
    where
        S: Iterator<Item = Spanned<Event<'a>>>,
    {
        let mut this = Self {
            source,
            rel_links: Default::default(),
            fragments: Default::default(),
        };

        struct Heading<'a> {
            span: Range<usize>,
            text: Vec<Event<'a>>,
        }

        let mut heading: Option<Heading<'_>> = None;
        let mut counter: HashMap<String, usize> = Default::default();

        let mut link: Option<RelativeLink<'_>> = None;

        for (event, span) in stream {
            match event {
                Event::Start(Tag::Heading { id, .. }) => {
                    if let Some(id) = id {
                        this.insert_id(&id, &mut counter);
                    } else if heading.is_some() {
                        bail!("unexpected `Tag::Heading` in `Tag::Heading` at {span:?}");
                    } else {
                        heading = Some(Heading { span, text: vec![] })
                    }
                }

                Event::End(TagEnd::Heading(..)) => {
                    let Some(Heading { span: start, text }) = heading.take() else {
                        bail!("unexpected `TagEnd::Heading` at {span:?}")
                    };
                    if start != span {
                        bail!("mismatching span, expected {start:?}, got {span:?}")
                    }
                    this.slugify(text.iter(), &mut counter);
                }

                Event::Start(Tag::Link {
                    dest_url, title, ..
                }) => {
                    if link.is_some() {
                        bail!("unexpected `Tag::Link` in `Tag::Link` at {span:?}")
                    }
                    link = Some(RelativeLink {
                        span,
                        link: dest_url,
                        text: vec![],
                        title,
                        status: LinkStatus::External,
                    });
                }

                Event::End(TagEnd::Link) => {
                    let Some(link) = link.take() else {
                        bail!("unexpected `TagEnd::Link` at {span:?}")
                    };
                    if link.span != span {
                        bail!("mismatching span, expected {:?}, got {span:?}", link.span);
                    }
                    this.rel_links.push(link);
                }

                event => match (heading.as_mut(), link.as_mut()) {
                    (Some(heading), Some(link)) => {
                        heading.text.push(event.clone());
                        link.text.push(event);
                    }
                    (Some(heading), None) => {
                        heading.text.push(event);
                    }
                    (None, Some(link)) => {
                        link.text.push(event);
                    }
                    (None, None) => {}
                },
            }
        }

        Ok(this)
    }

    fn emit(&self) -> Result<String> {
        self.rel_links
            .iter()
            .filter_map(|link| {
                if !matches!(link.status, LinkStatus::Permalink) {
                    return None;
                }
                Tag::Link {
                    link_type: LinkType::Inline,
                    dest_url: link.link.clone(),
                    title: link.title.clone(),
                    id: CowStr::Borrowed(""),
                }
                .pipe(|tag| std::iter::once(Event::Start(tag)))
                .chain(link.text.iter().cloned())
                .chain(std::iter::once(Event::End(TagEnd::Link)))
                .pipe(|events| Some((events, link.span.clone())))
            })
            .pipe(|stream| PatchStream::new(self.source, stream))
            .into_string()?
            .pipe(Ok)
    }

    fn slugify<'r, S>(&mut self, heading: S, counter: &mut HashMap<String, usize>)
    where
        S: Iterator<Item = &'r Event<'r>>,
    {
        fn unmark<'a>(event: &'a Event<'_>) -> &'a str {
            match event {
                Event::Text(text) => text,
                Event::Code(text) => text,
                Event::InlineMath(text) => text,
                Event::DisplayMath(text) => text,
                Event::Html(html) => html,
                Event::InlineHtml(html) => html,
                Event::FootnoteReference(href) => href,
                _ => "",
            }
        }
        let fragment = heading.map(unmark).collect::<String>();
        let fragment = unique_id_from_content(&fragment, counter);
        self.fragments.insert(fragment);
    }

    fn insert_id(&mut self, id: &str, counter: &mut HashMap<String, usize>) {
        counter.insert(id.into(), 1);
        self.fragments.insert(id.into());
    }
}

struct Fragments(HashMap<Arc<Url>, HashSet<String>>);

impl Fragments {
    fn restore(&mut self, pages: &mut Pages<'_>) {
        let fragments = std::mem::take(&mut self.0);
        for (url, items) in fragments {
            pages.pages.get_mut(&url).unwrap().fragments = items;
        }
    }
}

/// Drop bomb
impl Drop for Fragments {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            unreachable!("page fragments were not restored")
        }
    }
}
