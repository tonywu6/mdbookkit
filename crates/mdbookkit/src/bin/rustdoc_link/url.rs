use std::path::PathBuf;

use anyhow::{bail, Result};
use lsp_types::Url;

/// [`Url::to_file_path()`] with an actual [`std::error::Error`].
pub trait UrlToPath {
    fn to_path(&self) -> Result<PathBuf>;
}

impl UrlToPath for Url {
    fn to_path(&self) -> Result<PathBuf> {
        match self.to_file_path() {
            Ok(path) => Ok(path),
            Err(()) => bail!("failed to convert {self} to a file path"),
        }
    }
}
