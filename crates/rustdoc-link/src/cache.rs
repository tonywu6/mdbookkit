use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

use anyhow::{anyhow, bail, Context, Result};
use lsp_types::Url;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tap::{Pipe, Tap, TapFallible};
use tokio::task::JoinSet;

use crate::{
    client::{Client, ItemLinks},
    env::Environment,
    item::Item,
    log_debug,
    logger::spinner,
    Resolved,
};

#[allow(async_fn_in_trait)]
pub trait Caching: DeserializeOwned + Serialize {
    async fn reuse(self, env: &Environment, req: &[Item]) -> Result<Resolved>;
    async fn build(env: &Environment, map: &Resolved) -> Result<Self>;
}

#[allow(async_fn_in_trait)]
pub trait Cacheable<'a> {
    async fn cached<C: Caching>(self, this: &'a Client, request: Vec<Item>) -> Result<Resolved>;
}

impl<'a, F, R> Cacheable<'a> for F
where
    // AsyncFnOnce
    F: FnOnce(&'a Client, Vec<Item>) -> R,
    R: Future<Output = Result<Resolved>>,
{
    async fn cached<C: Caching>(self, this: &'a Client, request: Vec<Item>) -> Result<Resolved> {
        let cached = if let Ok(cache) = this
            .env
            .read_cache::<C>()
            .context("could not read cache")
            .tap_err(log_debug!())
        {
            cache
                .reuse(&this.env, &request)
                .await
                .context("could not reuse cache")
                .tap_err(log_debug!())
                .ok()
        } else {
            None
        };

        if let Some(cached) = cached {
            spinner().create("cache", None).finish("cache", "found");
            return Ok(cached);
        }

        let symbols = self(this, request).await?;

        if let Ok(cache) = C::build(&this.env, &symbols)
            .await
            .context("could not build cache")
            .tap_err(log_debug!())
        {
            this.env
                .save_cache(cache)
                .context("could not save cache")
                .tap_err(log_debug!())
                .ok();
        }

        Ok(symbols)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Cache {
    V1(CacheV1),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CacheV1 {
    hash: String,
    urls: HashMap<String, (Option<Url>, Option<Url>)>,
    deps: Vec<String>,
}

impl Caching for Cache {
    async fn reuse(self, env: &Environment, req: &[Item]) -> Result<Resolved> {
        match self {
            Self::V1(cache) => cache.reuse(env, req).await,
        }
    }

    async fn build(env: &Environment, map: &Resolved) -> Result<Self> {
        Ok(Self::V1(CacheV1::build(env, map).await?))
    }
}

impl Caching for CacheV1 {
    async fn reuse(self, _: &Environment, req: &[Item]) -> Result<Resolved> {
        let hash = JoinSet::<Result<(String, String)>>::new()
            .tap_mut(|tasks| {
                for dep in self.deps.iter() {
                    let Ok(dep) = dep.parse() else {
                        continue;
                    };
                    tasks.spawn(read_dep(dep));
                }
            })
            .join_all()
            .await
            .into_iter()
            .filter_map(|result| {
                result
                    .context("failed to read cache dependency")
                    .tap_err(log_debug!())
                    .ok()
            })
            .collect::<Vec<_>>()
            .tap_mut(|deps| deps.sort_by(|(k1, _), (k2, _)| k1.cmp(k2)))
            .into_iter()
            .fold(Sha256::new(), |mut hash, (_, src)| {
                hash.update(src);
                hash
            })
            .pipe(|hash| hash.finalize().digest());

        if hash != self.hash {
            bail!("checksum mismatch, expected {}, actual {hash}", self.hash)
        }

        let expected = req.iter().map(|item| &item.key).collect::<HashSet<_>>();
        let existing = self.urls.keys().collect::<HashSet<_>>();

        if !expected.is_subset(&existing) {
            return Err(anyhow!("expected  {expected:#?}, found {existing:#?}"))
                .context("could not reuse cache");
        }

        let items = self
            .urls
            .into_iter()
            .map(|(key, (web, local))| {
                (
                    key,
                    ItemLinks {
                        web,
                        local,
                        defs: vec![],
                    },
                )
            })
            .collect();

        Ok(Resolved { items })
    }

    async fn build(env: &Environment, res: &Resolved) -> Result<Self> {
        let (hash, deps) = JoinSet::<Result<(String, String)>>::new()
            .tap_mut(|tasks| {
                tasks.spawn(read_dep(env.crate_dir.join("Cargo.toml").unwrap()));
            })
            .tap_mut(|tasks| {
                if env.source_dir != env.crate_dir {
                    tasks.spawn(read_dep(env.source_dir.join("Cargo.toml").unwrap()));
                }
            })
            .tap_mut(|tasks| {
                for dep in res
                    .items
                    .iter()
                    .filter_map(|(_, sym)| {
                        if sym.is_empty() {
                            None
                        } else {
                            Some(sym.defs.iter())
                        }
                    })
                    .flatten()
                {
                    let Some(relpath) = env.source_dir.make_relative(dep) else {
                        continue;
                    };
                    if relpath.starts_with("../") {
                        continue;
                    }
                    tasks.spawn(read_dep(dep.clone()));
                }
            })
            .join_all()
            .await
            .into_iter()
            .filter_map(|result| {
                result
                    .context("failed to read cache dependency")
                    .tap_err(log_debug!())
                    .ok()
            })
            .collect::<Vec<_>>()
            .tap_mut(|deps| deps.sort_by(|(k1, _), (k2, _)| k1.cmp(k2)))
            .into_iter()
            .fold(
                (Sha256::new(), vec![]),
                |(mut hash, mut deps), (key, src)| {
                    deps.push(key);
                    hash.update(src);
                    (hash, deps)
                },
            )
            .pipe(|(hash, deps)| (hash.finalize().digest(), deps));

        let urls = res
            .items
            .iter()
            .map(|(k, s)| (k.clone(), (s.web.clone(), s.local.clone())))
            .collect::<HashMap<_, _>>();

        Ok(Self { hash, urls, deps })
    }
}

async fn read_dep(url: Url) -> Result<(String, String)> {
    let content = tokio::fs::read_to_string(&url.path()).await?;
    Ok((url.to_string(), content))
}

trait HexDigest {
    fn digest(&self) -> String;
}

impl HexDigest for sha2::digest::Output<Sha256> {
    fn digest(&self) -> String {
        format!("{:064x}", self)
    }
}
