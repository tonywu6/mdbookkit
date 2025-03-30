use anyhow::{bail, Context, Result};
use git2::{DescribeOptions, Repository};
use mdbook::preprocess::PreprocessorContext;
use tap::{Pipe, Tap, TapFallible};
use url::Url;

use crate::{
    env::{config_from_book, smart_punctuation},
    log_debug,
    markdown::mdbook_markdown,
};

use super::{Config, CustomPermalink, Environment, GitHubPermalink, PermalinkFormat};

impl Environment {
    pub fn from_book(book: &PreprocessorContext) -> Result<Self> {
        let repo = Repository::open_from_env()
            .context("preprocessor requires a git repository to work")
            .context("failed to find a git repository")?;

        let vcs_root = repo
            .workdir()
            .unwrap_or_else(|| repo.commondir())
            .pipe(Url::from_directory_path)
            .expect("failed to locate repo root");

        let book_src = book
            .root
            .join(&book.config.book.src)
            .pipe(Url::from_directory_path)
            .expect("book `src` should be a valid absolute path");

        let markdown = mdbook_markdown().tap_mut(|m| {
            if smart_punctuation(&book.config) {
                m.insert(pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION);
            }
        });

        let config = config_from_book::<Config>(&book.config, "link-forever")?;

        let Some(reference) =
            get_head(&repo).context("failed to get a tag or commit id to HEAD")?
        else {
            let fmt_link = Box::new(LocalFallbackLink {
                root: vcs_root.clone(),
            });
            return Ok(Self {
                book_src,
                vcs_root,
                fmt_link,
                markdown,
                config,
            });
        };

        let fmt_link: Box<dyn PermalinkFormat> = {
            if let Some(pat) = &config.url_pattern {
                CustomPermalink {
                    pattern: pat.clone(),
                    reference,
                }
                .pipe(Box::new)
            } else {
                find_github_repo(&repo, &book.config)
                    .context("help: use option `url-pattern` to provide a custom url format")
                    .context("help: set `output.html.git-repository-url` to a GitHub url")
                    .and_then(|repo| Ok(GitHubPermalink::new(&repo, &reference)?))
                    .context("failed to determine GitHub url")?
                    .pipe(Box::new)
            }
        };

        Ok(Self {
            book_src,
            vcs_root,
            fmt_link,
            markdown,
            config,
        })
    }
}

fn get_head(repo: &Repository) -> Result<Option<String>> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(err) => {
            log::debug!("{err}");
            log::info!("no commit yet, will not generate permalinks");
            return Ok(None);
        }
    };
    let head = head.peel_to_commit()?;
    if let Ok(tag) = head
        .as_object()
        .describe(
            DescribeOptions::new()
                .describe_tags()
                .max_candidates_tags(0), // exact match
        )
        .tap_err(log_debug!())
        .and_then(|tag| tag.format(None))
        .tap_err(log_debug!())
    {
        Ok(Some(tag))
    } else {
        Ok(Some(head.id().to_string()))
    }
}

fn find_github_repo(repo: &Repository, config: &mdbook::Config) -> Result<String> {
    let url = if let Some(url) = config
        .get_deserialized_opt::<String, _>("output.html.git-repository-url")
        .context("failed to get `output.html.git-repository-url`")?
    {
        gix_url::parse(url.as_str().into())
    } else {
        repo.find_remote("origin")
            .context("no such remote `origin`")?
            .url()
            .context("remote `origin` does not have a url")?
            .pipe(|url| gix_url::parse(url.into()))
    }
    .context("failed to parse remote url")?;

    let Some(host) = url.host() else {
        bail!("remote url does not have a host")
    };

    if host != "github.com" && !host.ends_with(".github.com") {
        bail!("unsupported remote {host:?}")
    }

    let path = url
        .path
        .to_string()
        .split('/')
        .skip_while(|c| c.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("/");

    match path.strip_suffix(".git") {
        Some(path) => path.to_owned(),
        None => path,
    }
    .pipe(Ok)
}

struct LocalFallbackLink {
    root: Url,
}

impl PermalinkFormat for LocalFallbackLink {
    fn link_to(&self, relpath: &str) -> Result<Url, url::ParseError> {
        self.root.join(relpath)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use git2::Repository;

    use super::find_github_repo;

    #[test]
    fn test_github_url_from_book() -> Result<()> {
        let config = r#"
        [output.html]
        git-repository-url = "https://github.com/lorem/ipsum/tree/main/crates/dolor"
        "#
        .parse::<mdbook::Config>()?;
        let repo = Repository::open_from_env()?;
        let url = find_github_repo(&repo, &config)?;
        assert_eq!(url, "lorem/ipsum");
        Ok(())
    }

    #[test]
    fn test_github_url_from_repo() -> Result<()> {
        let config = "".parse::<mdbook::Config>()?;
        let repo = Repository::open_from_env()?;
        let url = find_github_repo(&repo, &config)?;
        let (_, name) = url.split_once('/').unwrap();
        assert_eq!(name, env!("CARGO_PKG_NAME"));
        Ok(())
    }

    #[test]
    fn test_scp_uri() -> Result<()> {
        let config = r#"
        [output.html]
        git-repository-url = "git@my-alt.github.com:lorem/ipsum.git"
        "#
        .parse::<mdbook::Config>()?;
        let repo = Repository::open_from_env()?;
        let url = find_github_repo(&repo, &config)?;
        assert_eq!(url, "lorem/ipsum");
        Ok(())
    }

    #[test]
    #[should_panic(expected = "unsupported remote")]
    fn test_non_github() {
        let config = r#"
        [output.html]
        git-repository-url = "https://gitlab.haskell.org/ghc/ghc"
        "#
        .parse::<mdbook::Config>()
        .unwrap();
        let repo = Repository::open_from_env().unwrap();
        let _url = find_github_repo(&repo, &config).unwrap();
    }
}
