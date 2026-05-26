use std::path::PathBuf;

use anyhow::{Context, Result};
use mdbook_preprocessor::PreprocessorContext;
use url::Url;

use mdbookkit::{
    book::PreprocessorHelper,
    url::{UrlFromPath, UrlUtil},
};

use crate::options::EnvConfig;

#[derive(Debug)]
pub struct Environment {
    book_dir: Url,
    page_dir: Url,
    base_url: Url,
}

impl Environment {
    pub fn new(config: EnvConfig, book: &PreprocessorContext) -> Result<Self> {
        let book_dir = (book.root)
            .canonicalize()
            .context("could not locate book directory")?
            .dir_to_url();
        let page_dir = book.page_dir()?;
        let base_url = config.base_url.reify(&page_dir)?;
        Ok(Self {
            book_dir,
            page_dir,
            base_url,
        })
    }

    pub fn book_dir(&self) -> PathBuf {
        self.book_dir.expect_path()
    }

    pub fn page_dir(&self) -> &Url {
        &self.page_dir
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        if self.base_url.scheme() == "file" {
            Some(self.base_url.expect_path())
        } else {
            None
        }
    }
}

#[cfg(test)]
impl Default for Environment {
    fn default() -> Self {
        use crate::options::BaseUrl;

        let dir = std::env::current_dir().expect("current_dir should be accessible");
        Self {
            book_dir: dir.dir_to_url(),
            page_dir: dir.dir_to_url(),
            base_url: BaseUrl::default_url(),
        }
    }
}
