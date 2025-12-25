use std::{borrow::Borrow, collections::HashMap, fmt::Write, hash::Hash, sync::Arc};

use anyhow::{Context, Result};
use lsp_types::Position;
use tap::Pipe;
use tokio::task::JoinSet;
use tracing::{Instrument, Level, debug, info, instrument};

use mdbookkit::{ticker, ticker_item, url::UrlToPath};

use crate::{
    UNIQUE_ID,
    client::Client,
    item::Item,
    link::{ItemLinks, LinkState},
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
        let mut iter = pages.iter();

        let requests = iter.deduped(|link| match link.state() {
            LinkState::Pending(item) => Some(item),
            _ => None,
        });

        if iter.stats().has_pending() {
            info!("Resolving {}", iter.stats().fmt_pending());
        } else {
            debug!("no more items to resolve");
            return Ok(());
        }

        drop(iter);

        let main = self.env().entrypoint.expect_path();
        let main = std::fs::read_to_string(&main)
            .with_context(|| format!("Reading {}", main.display()))
            .context("Failed to read from crate entrypoint")?;

        let (context, request) = {
            let mut context = format!("{main}\nfn {UNIQUE_ID} () {{\n");

            let line = context.chars().filter(|&c| c == '\n').count();

            let request = requests
                .into_iter()
                .filter_map(|(k, v)| Some((k, v?)))
                .scan(line, |line, (key, item)| {
                    build(&mut context, line, item).map(|cursors| (key.clone(), cursors))
                })
                .collect::<Vec<_>>();

            fn build(context: &mut String, line: &mut usize, item: &Item) -> Option<Vec<Position>> {
                let _ = writeln!(context, "{}", item.stmt);
                let cursors = (item.cursor.as_ref().iter())
                    .map(|&col| Position::new(*line as _, col as _))
                    .collect::<Vec<_>>();
                *line += 1;
                Some(cursors)
            }

            context.push('}');

            (context, request)
        };

        debug!("synthesized function\n\n{context}\n");

        let document = self
            .open(self.env().entrypoint.clone(), context)
            .await?
            .pipe(Arc::new);

        info!("Finished indexing");

        let ticker = ticker!(
            Level::INFO,
            "resolve-items",
            count = request.len(),
            "resolving items"
        );

        let tasks: JoinSet<Option<(String, ItemLinks)>> = request
            .into_iter()
            .map(|(key, pos)| {
                let doc = document.clone();
                let key = key.to_string();
                let span = ticker_item!(&ticker, Level::INFO, "resolve", "{key:?}");
                async move {
                    for p in pos {
                        if let Ok(resolved) = doc.resolve(p).await {
                            return Some((key, resolved));
                        } else {
                            debug!("no result for {p:?}")
                        }
                    }
                    None
                }
                .instrument(span)
            })
            .collect();

        let resolved = tasks
            .join_all()
            .instrument(ticker)
            .await
            .into_iter()
            .flatten()
            .collect::<HashMap<_, _>>();

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
