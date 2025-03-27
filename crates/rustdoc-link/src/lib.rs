use std::{borrow::Borrow, collections::HashMap, hash::Hash, ops::Range, sync::Arc};

// not shadowing std::Result because rustdoc-link.md needs to link to it
use anyhow::{Context, Result as Result2};
use lsp_types::Position;
use tap::{Pipe, TapFallible};
use tokio::task::JoinSet;

pub mod cache;
mod client;
mod diagnostics;
pub mod env;
mod item;
mod link;
pub mod logger;
mod markdown;
mod page;
mod sync;

pub use crate::{client::Client, diagnostics::Status, page::Pages};
use crate::{
    item::Item,
    link::ItemLinks,
    logger::{spinner, styled},
};

/// Type that can provide links.
///
/// Resolvers should modify the provided [`Pages`] in place.
///
/// This is currently an abstraction over two sources of links:
///
/// - [`Client`], which invokes rust-analyzer
/// - [`Cache`][crate::cache::Cache] implementations
#[allow(async_fn_in_trait)]
pub trait Resolver {
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result2<()>
    where
        K: Eq + Hash;
}

impl Resolver for Client {
    async fn resolve<K>(&self, pages: &mut Pages<'_, K>) -> Result2<()>
    where
        K: Eq + Hash,
    {
        let request = pages.items();

        if request.is_empty() {
            return Ok(());
        }

        let main = std::fs::read_to_string(self.env().entrypoint.path())?;

        let (context, request) = {
            let mut context = format!("{main}\nfn {UNIQUE_ID} () {{\n");

            let line = context.chars().filter(|&c| c == '\n').count();

            let request = request
                .iter()
                .scan(line, |line, (key, item)| {
                    build(&mut context, line, item).map(|cursors| (key.clone(), cursors))
                })
                .collect::<Vec<_>>();

            fn build(context: &mut String, line: &mut usize, item: &Item) -> Option<Vec<Position>> {
                use std::fmt::Write;
                let _ = writeln!(context, "{}", item.stmt);
                let cursors = item
                    .cursor
                    .as_ref()
                    .iter()
                    .map(|&col| Position::new(*line as _, col as _))
                    .collect::<Vec<_>>();
                *line += 1;
                Some(cursors)
            }

            context.push('}');

            (context, request)
        };

        log::debug!("request context\n\n{context}\n");

        let document = self
            .open(self.env().entrypoint.clone(), context)
            .await?
            .pipe(Arc::new);

        spinner().create("resolve", Some(request.len() as _));

        let tasks: JoinSet<Option<(String, ItemLinks)>> = request
            .into_iter()
            .map(|(key, pos)| {
                let key = key.to_string();
                let doc = document.clone();
                resolve(doc, key, pos)
            })
            .collect();

        async fn resolve(
            doc: Arc<client::OpenDocument>,
            key: String,
            pos: Vec<Position>,
        ) -> Option<(String, ItemLinks)> {
            let _task = spinner().task("resolve", &key);
            for p in pos {
                let resolved = doc
                    .resolve(p)
                    .await
                    .with_context(|| format!("error resolving {p:?}"))
                    .tap_err(log_debug!())
                    .ok();
                if let Some(resolved) = resolved {
                    return Some((key, resolved));
                }
            }
            None
        }

        let resolved = tasks
            .join_all()
            .await
            .into_iter()
            .flatten()
            .collect::<HashMap<_, _>>();

        spinner().finish("resolve", styled("done").green());

        pages.apply(&resolved);

        Ok(())
    }
}

impl<K> Resolver for HashMap<K, ItemLinks>
where
    K: Borrow<str> + Eq + Hash,
{
    async fn resolve<P>(&self, pages: &mut Pages<'_, P>) -> Result2<()>
    where
        P: Eq + Hash,
    {
        pages.apply(self);
        Ok(())
    }
}

type Spanned<T> = (T, Range<usize>);

const UNIQUE_ID: &str = "__ded48f4d_0c4f_4950_b17d_55fd3b2a0c86__";

pub fn preprocessor_name() -> &'static str {
    let name = env!("CARGO_PKG_NAME");
    if let Some(idx) = name.find('-') {
        &name[idx + 1..]
    } else {
        name
    }
}
