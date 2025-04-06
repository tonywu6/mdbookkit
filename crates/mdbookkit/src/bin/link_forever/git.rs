use anyhow::{anyhow, bail, Context, Result};
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
    pub fn try_from_env(book: &PreprocessorContext) -> Result<Result<Self>> {
        let repo = match Repository::open_from_env()
            .context("preprocessor requires a git repository to work")
            .context("failed to find a git repository")
        {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };

        let vcs_root = repo
            .workdir()
            .unwrap_or_else(|| repo.commondir())
            .canonicalize()
            .context("failed to locate repo root")?
            .pipe(Url::from_directory_path)
            .map_err(|_| anyhow!("failed to locate repo root"))?;

        let book_src = book
            .root
            .canonicalize()
            .context("failed to locate book root")?
            .join(&book.config.book.src)
            .pipe(Url::from_directory_path)
            .map_err(|_| anyhow!("book `src` should be a valid absolute path"))?;

        let markdown = mdbook_markdown().tap_mut(|m| {
            if smart_punctuation(&book.config) {
                m.insert(pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION);
            }
        });

        let config = config_from_book::<Config>(&book.config, "link-forever")?;

        let Some(reference) =
            get_head(&repo).context("failed to get a tag or commit id to HEAD")?
        else {
            return Ok(Err(anyhow!("no commit found in this repo")));
        };

        let fmt_link: Box<dyn PermalinkFormat> = {
            if let Some(pat) = &config.repo_url_template {
                CustomPermalink {
                    pattern: pat
                        .parse()
                        .context("failed to parse `repo-url-template` as a valid url")?,
                    reference,
                }
                .pipe(Box::new)
            } else {
                let repo = match find_git_remote(&repo, &book.config)? {
                    Ok(repo) => repo,
                    Err(err) => {
                        return err
                            .context("help: or use `repo-url-template` option")
                            .context("help: set `output.html.git-repository-url` to a GitHub url")
                            .context("failed to determine GitHub url to use for permalinks")
                            .pipe(Err)
                            .pipe(Ok)
                    }
                };
                let (owner, repo) = remote_as_github(repo.as_ref())
                    .with_context(|| match repo {
                        RepoSource::Config(..) => "in `output.html.git-repository-url`",
                        RepoSource::Remote(..) => "from git remote \"origin\"",
                    })
                    .context("help: use `repo-url-template` option for a custom remote")
                    .context("failed to parse git remote url")?;
                GitHubPermalink::new(&owner, &repo, &reference).pipe(Box::new)
            }
        };

        Ok(Ok(Self {
            book_src,
            vcs_root,
            fmt_link,
            markdown,
            config,
        }))
    }
}

enum RepoSource {
    Config(gix_url::Url),
    Remote(gix_url::Url),
}

impl AsRef<gix_url::Url> for RepoSource {
    fn as_ref(&self) -> &gix_url::Url {
        match self {
            Self::Config(u) => u,
            Self::Remote(u) => u,
        }
    }
}

fn get_head(repo: &Repository) -> Result<Option<String>> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(err) => {
            log::debug!("{err}");
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

fn find_git_remote(repo: &Repository, config: &mdbook::Config) -> Result<Result<RepoSource>> {
    if let Some(url) = config
        .get_deserialized_opt::<String, _>("output.html.git-repository-url")
        .context("failed to get `output.html.git-repository-url`")?
    {
        gix_url::parse(url.as_str().into())?
            .pipe(RepoSource::Config)
            .pipe(Ok)
            .pipe(Ok)
    } else {
        let repo = match repo
            .find_remote("origin")
            .context("no such remote `origin`")
        {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };
        let repo = match repo.url() {
            Some(url) => url,
            None => return Ok(Err(anyhow!("remote `origin` does not have a url"))),
        };
        gix_url::parse(repo.into())?
            .pipe(RepoSource::Remote)
            .pipe(Ok)
            .pipe(Ok)
    }
}

fn remote_as_github(url: &gix_url::Url) -> Result<(String, String)> {
    let Some(host) = url.host() else {
        bail!("remote url does not have a host")
    };

    if host != "github.com" && !host.ends_with(".github.com") {
        bail!("unsupported remote {host:?}, only `github.com` is supported")
    }

    let path = url.path.to_string();

    let mut iter = path.split('/').skip_while(|c| c.is_empty()).take(2);

    let owner = iter
        .next()
        .with_context(|| format!("malformed path {path:?}, expected `/<owner>/<repo>`"))?;

    let repo = iter
        .next()
        .with_context(|| format!("malformed path {path:?}, expected `/<owner>/<repo>`"))?;

    let repo = repo.strip_suffix(".git").unwrap_or(repo);

    Ok((owner.to_owned(), repo.to_owned()))
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use git2::Repository;

    use super::{find_git_remote, remote_as_github};

    #[test]
    fn test_github_url_from_book() -> Result<()> {
        let config = r#"
        [output.html]
        git-repository-url = "https://github.com/lorem/ipsum/tree/main/crates/dolor"
        "#
        .parse::<mdbook::Config>()?;
        let repo = Repository::open_from_env()?;
        let repo = find_git_remote(&repo, &config)??;
        let (owner, repo) = remote_as_github(repo.as_ref())?;
        assert_eq!(owner, "lorem");
        assert_eq!(repo, "ipsum");
        Ok(())
    }

    #[test]
    fn test_github_url_from_repo() -> Result<()> {
        let config = "".parse::<mdbook::Config>()?;
        let repo = Repository::open_from_env()?;
        let repo = find_git_remote(&repo, &config)??;
        let (_, repo) = remote_as_github(repo.as_ref())?;
        assert_eq!(repo, env!("CARGO_PKG_NAME"));
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
        let repo = find_git_remote(&repo, &config)??;
        let (owner, repo) = remote_as_github(repo.as_ref())?;
        assert_eq!(owner, "lorem");
        assert_eq!(repo, "ipsum");
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
        let repo = find_git_remote(&repo, &config).unwrap().unwrap();
        let _ = remote_as_github(repo.as_ref()).unwrap();
    }
}
