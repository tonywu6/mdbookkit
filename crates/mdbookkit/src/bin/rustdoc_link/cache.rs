use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    hash::Hash,
    iter,
};

use anyhow::{bail, Context, Result};
use lsp_types::Url;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tap::{Pipe, Tap, TapFallible};
use tokio::task::JoinSet;

use crate::log_debug;

use super::{env::Environment, link::ItemLinks, page::Pages, url::UrlToPath, Resolver};

#[allow(async_fn_in_trait)]
pub trait Cache: DeserializeOwned + Serialize {
    type Validated: Resolver;

    async fn reuse(self, env: &Environment) -> Result<Self::Validated>;

    async fn build<K>(env: &Environment, content: &Pages<'_, K>) -> Result<Self>
    where
        K: Eq + Hash;

    async fn load(env: &Environment) -> Result<Self::Validated> {
        env.load_temp::<Self, _>("cache.json")
            .tap_err(log_debug!())?
            .reuse(env)
            .await
            .tap_err(log_debug!())
    }

    async fn save<K>(env: &Environment, content: &Pages<'_, K>) -> Result<()>
    where
        K: Eq + Hash,
    {
        let this = Self::build(env, content).await?;
        env.save_temp::<Self, _>("cache.json", &this)
            .tap_err(log_debug!())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileCache {
    V1(FileCacheV1),
}

impl Cache for FileCache {
    type Validated = HashMap<String, ItemLinks>;

    async fn reuse(self, env: &Environment) -> Result<Self::Validated> {
        match self {
            Self::V1(cache) => Ok(cache.reuse(env).await?),
        }
    }

    async fn build<K>(env: &Environment, content: &Pages<'_, K>) -> Result<Self>
    where
        K: Eq + Hash,
    {
        Ok(Self::V1(FileCacheV1::build(env, content).await?))
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileCacheV1 {
    hash: String,
    urls: Vec<(String, ItemLinks)>,
}

impl Cache for FileCacheV1 {
    type Validated = HashMap<String, ItemLinks>;

    async fn reuse(self, env: &Environment) -> Result<Self::Validated> {
        let deps = self
            .urls
            .iter()
            .flat_map(|(_, links)| links.deps())
            .map(Cow::Borrowed);

        let hash = Self::hash(env, deps).await;

        if hash != self.hash {
            bail!("checksum mismatch, expected {}, actual {hash}", self.hash)
        }

        Ok(self.urls.into_iter().collect())
    }

    async fn build<K>(env: &Environment, content: &Pages<'_, K>) -> Result<Self>
    where
        K: Eq + Hash,
    {
        let urls = content
            .links()
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<Vec<_>>();

        let deps = urls
            .iter()
            .flat_map(|(_, links)| links.deps())
            .map(Cow::Borrowed);

        let hash = Self::hash(env, deps).await;

        Ok(Self { hash, urls })
    }
}

impl FileCacheV1 {
    async fn hash<'a, D>(env: &'a Environment, deps: D) -> String
    where
        D: Iterator<Item = Cow<'a, Url>>,
    {
        iter::once(Cow::Owned(env.source_dir.join("Cargo.toml").unwrap()))
            .chain(iter::once(Cow::Owned(
                env.crate_dir.join("Cargo.toml").unwrap(),
            )))
            .chain(iter::once(Cow::Borrowed(&env.entrypoint)))
            .chain(deps)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|dep| {
                if env.source_dir.make_relative(&dep)?.starts_with("../") {
                    None
                } else {
                    Some(dep.into_owned())
                }
            })
            .map(read_dep)
            .collect::<JoinSet<_>>()
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
            .pipe(|hash| hash.finalize().digest())
    }
}

async fn read_dep(url: Url) -> Result<(String, String)> {
    let content = tokio::fs::read_to_string(&url.to_path()?).await?;
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
