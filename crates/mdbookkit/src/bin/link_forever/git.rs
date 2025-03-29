use url::Url;

use super::PermalinkScheme;

pub struct GitHubPermalink {
    prefix: Url,
}

impl PermalinkScheme for GitHubPermalink {
    fn link_to(&self, relpath: &str) -> Result<Url, url::ParseError> {
        self.prefix.join(relpath)
    }
}

impl GitHubPermalink {
    pub fn new(owner: &str, repo: &str, ref_: &str) -> Result<Self, url::ParseError> {
        let prefix = format!("https://github.com/{owner}/{repo}/tree/{ref_}/").parse()?;
        Ok(Self { prefix })
    }
}
