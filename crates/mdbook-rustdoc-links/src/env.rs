use anyhow::Result;
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use mdbook_preprocessor::PreprocessorContext;

use mdbookkit::{book::PreprocessorHelper, url::ToUtf8Path};

use crate::options::{BaseUrl, EnvConfig};

#[derive(Debug, Default)]
pub struct Environment {
    book_dir: Utf8PathBuf,
    src_path: Utf8PathBuf,
    config: EnvConfig,
}

impl Environment {
    pub fn new(config: EnvConfig, book: &PreprocessorContext) -> Result<Self> {
        Ok(Self {
            book_dir: book.root.as_path().into_utf8_path_buf()?,
            src_path: book.src_path()?,
            config,
        })
    }

    pub fn base_url(&self) -> &BaseUrl {
        &self.config.base_url
    }

    pub fn base_dir(&self) -> Option<Utf8PathBuf> {
        if !self.base_url().0.is_url() {
            let path = (self.base_url().0.as_str())
                .trim_start_matches('/')
                .trim_end_matches('/');
            Some(self.src_path.join(path))
        } else {
            None
        }
    }

    pub fn book_dir(&self) -> &Utf8Path {
        &self.book_dir
    }
}
