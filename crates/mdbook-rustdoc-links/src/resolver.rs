use std::{borrow::Borrow, collections::HashMap, hash::Hash, sync::Arc};

use anyhow::{Context, Result};
use futures_util::TryFutureExt;
use lsp_types::Url;
use mdbook_markdown::pulldown_cmark::{CowStr, Event, Parser, Tag};
use tap::Pipe;
use tracing::{Level, debug, info, instrument};

use mdbookkit::{
    emit_debug, error::FutureWithError, markdown::default_markdown_options, ticker, ticker_event,
    ticker_item, url::UrlToPath, write_str,
};

use crate::{
    UNIQUE_ID,
    client::Client,
    link::{ItemLinks, LinkState},
    markup::AttributedString,
    page::{PageKey, Pages},
};

/// Type that can provide links.
///
/// Resolvers should modify the provided [`Pages`] in place.
///
/// This is currently an abstraction over two sources of links:
///
/// - [`Client`], which invokes rust-analyzer
/// - [`Cache`] implementations
///
/// [`Cache`]: crate::cache::Cache
pub trait Resolver {
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result<()>
    where
        K: PageKey;
}

impl Resolver for Client {
    #[instrument(level = "debug", skip_all)]
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result<()>
    where
        K: PageKey,
    {
        let items = {
            let mut iter = pages.iter();

            let items = iter
                .deduped(|link| match link.state() {
                    LinkState::Pending(item) => Some(item),
                    _ => None,
                })
                .into_iter()
                .filter_map(|(k, v)| Some((k, v?)))
                .collect::<Vec<_>>();

            if iter.stats().has_pending() {
                info!("Resolving {}", iter.stats().fmt_pending());
                items
            } else {
                debug!("no more items to resolve");
                return Ok(());
            }
        };

        let text = self.env().entrypoint.expect_path();
        let text = std::fs::read_to_string(&text)
            .with_context(|| format!("Reading {}", text.display()))
            .context("Failed to read from crate entrypoint")?;

        #[derive(Debug, PartialEq, Eq, Hash)]
        enum Cursor<'a> {
            DocString,
            Definition(CowStr<'a>),
            ExternalDocs(CowStr<'a>),
        }

        let (text, markups) = {
            let mut source = AttributedString::from(text);

            write_str!(source, "\n");

            for (key, _) in items.iter() {
                write_str!(source, "/// ");
                source.markup(Cursor::Definition(key.clone()));
                write_str!(source, "[{key}]({key})\n");
                write_str!(source, "///\n");
            }

            write_str!(source, "fn ");
            source.markup(Cursor::DocString);
            write_str!(source, "{UNIQUE_ID}() {{\n");

            for (key, item) in items.iter() {
                let statement = (item.statement)
                    .clone()
                    .map(|_| Cursor::ExternalDocs(key.clone()));
                write_str!(source, "    ");
                source.append(statement);
                write_str!(source, "\n");
            }

            write_str!(source, "}}\n");
            source.into_parts()
        };

        debug!("synthesized document\n{text}");

        let document = self
            .open(self.env().entrypoint.clone(), text)
            .await?
            .pipe(Arc::new);

        info!("Finished indexing");

        let ticker = ticker!(
            Level::INFO,
            "resolve-items",
            count = items.len(),
            "resolving items"
        )
        .entered();

        let docstring = markups
            .get(&Cursor::DocString)
            .and_then(|pos| pos.first())
            .unwrap_or_else(|| unreachable!());

        let docstring = (document.hover(*docstring))
            .context("Error while rendering docstring")
            .inspect_ok(emit_debug!("rendered docstring\n{}"))
            .inspect_err(emit_debug!())
            .await
            .unwrap_or_default();

        let mut resolved = Parser::new_ext(&docstring, default_markdown_options())
            .scan(None, |state, event| match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    *state = dest_url.parse::<Url>().ok();
                    Some(None)
                }
                Event::Text(key) => {
                    if let Some(url) = state.take()
                        && let Ok(link) = ItemLinks::new(Some(url), None)
                    {
                        let _span = ticker_item!(&ticker, Level::INFO, "docs", item = &*key);
                        Some(Some((key.to_string(), link)))
                    } else {
                        Some(None)
                    }
                }
                _ => Some(None),
            })
            .flatten()
            .collect::<HashMap<_, _>>();

        for (key, pos) in markups.iter() {
            let Cursor::ExternalDocs(key) = key else {
                continue;
            };
            if let Some(link) = resolved.get(&**key)
            // TODO: despite https://github.com/rust-lang/rust-analyzer/pull/20384 links
            // generated for macros via hover are still wrong, hence we fallback to `externalDocs`
                && !link.url().path().contains("/macro.")
            {
                continue;
            }
            let _span = ticker_item!(&ticker, Level::INFO, "docs", item = &**key);
            for p in pos {
                if let Ok(link) = (document.external_docs(*p))
                    .with_context(|| format!("Error while resolving external docs at {p:?}"))
                    .inspect_err(emit_debug!())
                    .await
                {
                    resolved.insert(key.to_string(), link);
                    break;
                }
            }
        }

        for (key, pos) in markups.iter() {
            let Cursor::Definition(key) = key else {
                continue;
            };
            let Some(link) = resolved.get_mut(&**key) else {
                continue;
            };
            ticker_event!(&ticker, Level::DEBUG, item = ?&**key, "resolving definition");
            for p in pos {
                if let Ok(defs) = (document.definitions(*p))
                    .context("Error while resolving item definition")
                    .inspect_err(emit_debug!())
                    .await
                {
                    link.set_dependencies(defs);
                    break;
                }
            }
        }

        pages.apply(&resolved);

        Ok(())
    }
}

impl<K> Resolver for HashMap<K, ItemLinks>
where
    K: Borrow<str> + Eq + Hash,
{
    async fn resolve<P>(&self, pages: &mut Pages<'_, P>) -> Result<()>
    where
        P: PageKey,
    {
        pages.apply(self);
        Ok(())
    }
}
