use std::path::{Path, PathBuf};

use anyhow::Result;
use mdbook_preprocessor::PreprocessorContext;
use url::Url;

use mdbookkit::{book::PreprocessorHelper, url::UrlFromPath};

use crate::options::EnvConfig;

#[derive(Debug)]
pub struct Environment {
    book_dir: PathBuf,
    page_dir: Url,
    base_url: Url,
    base_dir: Option<PathBuf>,
}

impl Environment {
    pub fn new(config: EnvConfig, book: &PreprocessorContext) -> Result<Self> {
        let book_dir = book.book_dir()?;
        let page_dir = book.page_dir()?;
        let (base_url, base_dir) = config.base_url.take().resolve(page_dir.clone());
        let page_dir = page_dir.dir_to_url();
        Ok(Self {
            book_dir,
            page_dir,
            base_url,
            base_dir,
        })
    }

    pub fn book_dir(&self) -> &Path {
        &self.book_dir
    }

    pub fn page_dir(&self) -> &Url {
        &self.page_dir
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }
}

#[cfg(test)]
impl Default for Environment {
    fn default() -> Self {
        use crate::options::BaseUrl;
        let dir = std::env::current_dir().unwrap();
        let (base_url, base_dir) = BaseUrl::default().resolve(dir.clone());
        Self {
            page_dir: dir.dir_to_url(),
            book_dir: dir,
            base_url,
            base_dir,
        }
    }
}
