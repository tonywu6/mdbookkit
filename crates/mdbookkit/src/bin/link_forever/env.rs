use anyhow::Result;
use serde::Deserialize;
use url::Url;

use crate::env::ErrorHandling;

use super::PermalinkFormat;

#[cfg(feature = "link-forever")]
mod git;

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub url_pattern: Option<String>,

    #[serde(default)]
    pub always_link: Vec<String>,

    #[serde(default)]
    pub fail_on_unresolved: ErrorHandling,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub after: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub before: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub renderers: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    pub command: Option<String>,
}

pub struct Environment {
    pub book_src: Url,
    pub vcs_root: Url,
    pub fmt_link: Box<dyn PermalinkFormat>,
    pub markdown: pulldown_cmark::Options,
    pub config: Config,
}

pub struct GitHubPermalink {
    prefix: Url,
}

impl PermalinkFormat for GitHubPermalink {
    fn link_to(&self, relpath: &str) -> Result<Url, url::ParseError> {
        self.prefix.join(relpath)
    }
}

impl GitHubPermalink {
    pub fn new(path: &str, reference: &str) -> Result<Self, url::ParseError> {
        let prefix = format!("https://github.com/{path}/tree/{reference}/").parse()?;
        Ok(Self { prefix })
    }
}

pub struct CustomPermalink {
    pub pattern: String,
    pub reference: String,
}

impl PermalinkFormat for CustomPermalink {
    fn link_to(&self, relpath: &str) -> Result<Url, url::ParseError> {
        self.pattern
            .replace("{ref}", &self.reference)
            .replace("{path}", relpath)
            .parse()
    }
}
