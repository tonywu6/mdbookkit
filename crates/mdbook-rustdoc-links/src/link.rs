use std::{borrow::Cow, ops::Range, sync::Arc};

use anyhow::{Result, bail};
use lsp_types::Url;
use mdbook_markdown::pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use tap::{Pipe, Tap, TapFallible};

use mdbookkit::log_trace;

use crate::{env::EmitConfig, item::Item};

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
    Parsed(Item),
    Resolved(ItemLinks),
}

impl<'a> Link<'a> {
    pub fn new(span: Range<usize>, url: CowStr<'a>, title: CowStr<'a>) -> Self {
        let path = match url.split_once('#') {
            None => &url,
            Some((path, _)) => path,
        };

        let state = Item::parse(path)
            .tap_err(log_trace!())
            .ok()
            .map(LinkState::Parsed)
            .unwrap_or(LinkState::Unparsed);

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

    pub fn state(&mut self) -> &mut LinkState {
        &mut self.state
    }

    pub fn inner(&mut self) -> &mut Vec<Event<'a>> {
        &mut self.inner
    }

    pub fn item(&self) -> Option<&Item> {
        if let LinkState::Parsed(item) = &self.state {
            Some(item)
        } else {
            None
        }
    }

    pub fn link(&self) -> Option<ItemLinks> {
        if let LinkState::Resolved(item) = &self.state {
            Some(item.clone())
        } else {
            None
        }
    }

    pub fn emit(&self, options: &EmitConfig) -> Option<(__emit::EmitLink<'_>, Range<usize>)> {
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
            refs.local().or(refs.web())
        } else {
            refs.web()
        }?;
        if let Some(frag) = self.fragment() {
            url.clone()
                .tap_mut(|u| u.set_fragment(Some(frag)))
                .pipe(Cow::<Url>::Owned)
                .pipe(Some)
        } else {
            Some(Cow::Borrowed(url))
        }
    }

    fn fragment(&self) -> Option<&str> {
        self.url.split_once('#').map(|split| split.1)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct ItemLinks {
    refs: Locations,
    defs: Vec<Arc<Url>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Locations {
    Http { http: Arc<Url> },
    File { file: Arc<Url> },
    Multiple { http: Arc<Url>, file: Arc<Url> },
}

impl ItemLinks {
    pub fn new(http: Option<Url>, file: Option<Url>, defs: Vec<Url>) -> Result<Self> {
        let refs = match (http, file) {
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
            (None, None) => bail!("neither web nor local link provided"),
        };
        let defs = defs.into_iter().map(Into::into).collect();
        Ok(Self { refs, defs })
    }

    pub fn url(&self) -> &Url {
        match &self.refs {
            Locations::Http { http } => http,
            Locations::File { file } => file,
            Locations::Multiple { http, .. } => http,
        }
    }

    pub fn web(&self) -> Option<&Url> {
        match &self.refs {
            Locations::Http { http } => Some(http),
            Locations::File { .. } => None,
            Locations::Multiple { http, .. } => Some(http),
        }
    }

    pub fn local(&self) -> Option<&Url> {
        match &self.refs {
            Locations::Http { .. } => None,
            Locations::File { file } => Some(file),
            Locations::Multiple { file, .. } => Some(file),
        }
    }

    pub fn deps(&self) -> impl Iterator<Item = &'_ Url> {
        self.defs.iter().map(|u| u.as_ref())
    }
}

mod __emit {
    use std::{
        iter::{Chain, Cloned, Once},
        slice::Iter,
    };

    use mdbook_markdown::pulldown_cmark::Event;

    pub type EmitLink<'a> =
        Chain<Chain<Once<Event<'a>>, Cloned<Iter<'a, Event<'a>>>>, Once<Event<'a>>>;
}
