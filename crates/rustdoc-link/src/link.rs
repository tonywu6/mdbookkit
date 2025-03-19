use std::{borrow::Cow, ops::Range, sync::Arc};

use anyhow::{bail, Result};
use lsp_types::Url;
use pulldown_cmark::{CowStr, Event};
use serde::{Deserialize, Serialize};
use tap::{Pipe, Tap};

use crate::{env::EmitConfig, log_trace, Item};

#[derive(Debug)]
pub struct Link<'a> {
    pub span: Range<usize>,
    pub url: CowStr<'a>,
    pub state: LinkState,
    pub title: CowStr<'a>,
    pub inner: Vec<Event<'a>>,
}

#[derive(Debug, Default)]
pub enum LinkState {
    #[default]
    Untouched,
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
            .tap_mut(log_trace!())
            .ok()
            .map(LinkState::Parsed)
            .unwrap_or_default();

        let inner = vec![];

        Self {
            span,
            url,
            state,
            title,
            inner,
        }
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

    pub fn emit(&self, options: &EmitConfig) -> Option<Cow<'_, Url>> {
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
            (None, None) => bail!("doc links not resolved"),
        };
        let defs = defs.into_iter().map(Into::into).collect();
        Ok(Self { refs, defs })
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
