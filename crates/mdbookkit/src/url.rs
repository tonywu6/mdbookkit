use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use url::Url;

pub trait ExpectUrl<T> {
    fn expect_url(self) -> T;
}

impl<T> ExpectUrl<T> for Result<T, url::ParseError> {
    #[inline(always)]
    fn expect_url(self) -> T {
        self.expect("should be a valid URL")
    }
}

pub trait UrlToPath {
    fn to_path(&self) -> Result<PathBuf>;

    fn expect_path(&self) -> PathBuf;
}

impl UrlToPath for Url {
    #[inline(always)]
    fn to_path(&self) -> Result<PathBuf> {
        match self.to_file_path() {
            Ok(path) => Ok(path),
            Err(_) => bail!("{self} does not have a valid file path"),
        }
    }

    #[inline(always)]
    fn expect_path(&self) -> PathBuf {
        self.to_path().expect("URL path should be valid")
    }
}

pub trait UrlFromPath {
    fn to_directory_url(&self) -> Url;

    fn to_file_url(&self) -> Url;
}

impl<P: AsRef<Path> + ?Sized> UrlFromPath for P {
    #[inline(always)]
    fn to_directory_url(&self) -> Url {
        Url::from_directory_path(self).expect("should be a valid absolute path")
    }

    #[inline(always)]
    fn to_file_url(&self) -> Url {
        Url::from_file_path(self).expect("should be a valid absolute path")
    }
}
