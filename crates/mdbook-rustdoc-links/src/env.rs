use std::path::PathBuf;

use anyhow::Result;
use mdbook_preprocessor::PreprocessorContext;

use mdbookkit::book::PreprocessorHelper;

use crate::options::{BaseUrl, EnvConfig};

#[derive(Debug, Default)]
pub struct Environment {
    src_root: PathBuf,
    config: EnvConfig,
}

impl Environment {
    pub fn new(config: EnvConfig, book: &PreprocessorContext) -> Result<Self> {
        let src_root = book.src_root()?;
        Ok(Self { src_root, config })
    }

    pub fn base_url(&self) -> &BaseUrl {
        &self.config.base_url
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        if self.base_url().0.as_url().is_none() {
            let path = (self.base_url().0.as_str())
                .trim_start_matches('/')
                .trim_end_matches('/');
            Some(self.src_root.join(path))
        } else {
            None
        }
    }
}
