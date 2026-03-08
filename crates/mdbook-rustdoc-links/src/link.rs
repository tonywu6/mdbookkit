use std::{borrow::Cow, ops::Range, sync::Arc};

use anyhow::{Result, bail};
use lsp_types::Url;
use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use tap::{Pipe, Tap};
use tracing::trace;

use mdbookkit::markdown::Spanned;

use crate::{env::EmitConfig, item::Item, markdown::split_once};

pub mod diagnostic;

#[derive(Debug)]
pub struct Link<'a> {
    span: Range<usize>,
    url: CowStr<'a>,
    state: LinkState,
    title: CowStr<'a>,
    inner: Vec<Event<'a>>,
}

#[derive(Debug)]
pub enum LinkState {
    Unparsed,
    Pending(Item),
    Resolved(ItemLinks),
}

impl<'a> Link<'a> {
    pub fn new(span: Range<usize>, url: CowStr<'a>, title: CowStr<'a>) -> Self {
        let state = {
            let name = split_fragment(url.clone()).0;

            let name = match split_once(name, '@') {
                (_, Some(name)) => name,
                (name, None) => name,
            };

            match Item::new(&name) {
                Ok(item) => LinkState::Pending(item),
                Err(err) => {
                    trace!("{err:?}");
                    LinkState::Unparsed
                }
            }
        };

        let inner = vec![];

        Self {
            span,
            url,
            state,
            title,
            inner,
        }
    }

    pub fn span(&self) -> &Range<usize> {
        &self.span
    }

    pub fn key(&self) -> &CowStr<'a> {
        &self.url
    }

    pub fn state(&self) -> &LinkState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut LinkState {
        &mut self.state
    }

    pub fn inner_mut(&mut self) -> &mut Vec<Event<'a>> {
        &mut self.inner
    }

    pub fn emit(&self, options: &EmitConfig) -> Option<Spanned<impl Iterator<Item = Event<'_>>>> {
        Tag::Link {
            dest_url: self.url(options)?.to_string().into(),
            link_type: LinkType::Inline,
            title: self.title.clone(),
            id: CowStr::Borrowed(""),
        }
        .pipe(|tag| std::iter::once(Event::Start(tag)))
        .chain(self.inner.iter().cloned())
        .chain(std::iter::once(Event::End(TagEnd::Link)))
        .pipe(|events| Some((events, self.span().clone())))
    }

    fn url(&self, options: &EmitConfig) -> Option<Cow<'_, Url>> {
        let LinkState::Resolved(refs) = &self.state else {
            return None;
        };
        let url = if options.prefer_local_links {
            refs.file_url().or(refs.http_url())
        } else {
            refs.http_url()
        }?;
        if let Some(frag) = split_fragment(self.url.clone()).1 {
            url.clone()
                .tap_mut(|u| u.set_fragment(Some(&*frag)))
                .pipe(Cow::<Url>::Owned)
                .pipe(Some)
        } else {
            Some(Cow::Borrowed(url))
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct ItemLinks {
    docs: Locations,
    deps: Vec<Arc<Url>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Locations {
    Http { http: Arc<Url> },
    File { file: Arc<Url> },
    Multiple { http: Arc<Url>, file: Arc<Url> },
}

impl ItemLinks {
    pub fn new(http: Option<Url>, file: Option<Url>) -> Result<Self> {
        let docs = match (http, file) {
            (Some(http), Some(file)) => Locations::Multiple {
                http: Arc::new(http),
                file: Arc::new(file),
            },
            (Some(http), None) => Locations::Http {
                http: Arc::new(http),
            },
            (None, Some(file)) => Locations::File {
                file: Arc::new(file),
            },
            (None, None) => bail!("Neither web nor local link provided"),
        };
        let deps = Default::default();
        Ok(Self { docs, deps })
    }

    pub fn url(&self) -> &Url {
        match &self.docs {
            Locations::Http { http } => http,
            Locations::File { file } => file,
            Locations::Multiple { http, .. } => http,
        }
    }

    pub fn http_url(&self) -> Option<&Url> {
        match &self.docs {
            Locations::Http { http } => Some(http),
            Locations::File { .. } => None,
            Locations::Multiple { http, .. } => Some(http),
        }
    }

    pub fn file_url(&self) -> Option<&Url> {
        match &self.docs {
            Locations::Http { .. } => None,
            Locations::File { file } => Some(file),
            Locations::Multiple { file, .. } => Some(file),
        }
    }

    pub fn dependencies(&self) -> impl Iterator<Item = &Url> {
        self.deps.iter().map(|u| u.as_ref())
    }

    pub fn set_dependencies(&mut self, deps: Vec<Url>) {
        self.deps = deps.into_iter().map(Arc::new).collect();
    }
}

/// Split fragment from `url` by finding the '#' character.
///
/// This function does not handle raw identifiers like `r#type` correctly, but it turns out
/// rustdoc doesn't correctly parse raw identifiers either, and authors should simply
/// write the identifier without the `r#` part.
fn split_fragment<'a>(url: CowStr<'a>) -> (CowStr<'a>, Option<CowStr<'a>>) {
    split_once(url, '#')
}
