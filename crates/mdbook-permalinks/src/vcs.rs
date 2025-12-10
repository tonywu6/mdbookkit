use std::{collections::HashMap, ops::ControlFlow};

use anyhow::{Context, Result, anyhow, bail};
use git2::{DescribeOptions, Repository};
use mdbook_preprocessor::config::Config as MDBookConfig;
use tap::{Pipe, Tap, TapFallible};
use url::{Url, form_urlencoded::Serializer as SearchParams};

use mdbookkit::log_debug;

use crate::{Config, VersionControl};

impl VersionControl {
    pub fn try_from_git(config: &Config, book: &MDBookConfig) -> Result<Result<Self>> {
        let repo = match Repository::open_from_env()
            .context("preprocessor requires a git repository to work")
            .context("failed to find a git repository")
        {
            Ok(repo) => repo,
            Err(err) => return config.fail_on_warnings.adjusted(Ok(Err(err))),
        };

        let root = repo
            .workdir()
            .unwrap_or_else(|| repo.commondir())
            .canonicalize()
            .context("failed to locate repo root")?
            .pipe(Url::from_directory_path)
            .map_err(|_| anyhow!("failed to locate repo root"))?;

        let Some(reference) =
            get_git_head(&repo).context("failed to get a tag or commit id to HEAD")?
        else {
            return config
                .fail_on_warnings
                .adjusted(Ok(Err(anyhow!("no commit found in this repo"))));
        };

        let link = {
            if let Some(pat) = &config.repo_url_template {
                CustomPermalink {
                    pattern: pat
                        .parse()
                        .context("failed to parse `repo-url-template` as a valid url")?,
                    reference,
                }
                .pipe(Permalink::Custom)
            } else {
                let repo = match find_git_remote(&repo, book)? {
                    Ok(repo) => repo,
                    Err(err) => {
                        return err
                            .context("help: or use `repo-url-template` option")
                            .context("help: set `output.html.git-repository-url` to a GitHub url")
                            .context("failed to determine GitHub url to use for permalinks")
                            .pipe(Err)
                            .pipe(Ok)
                            .pipe(|result| config.fail_on_warnings.adjusted(result));
                    }
                };
                let (owner, repo) = remote_as_github(repo.as_ref())
                    .with_context(|| match repo {
                        RepoSource::Config(..) => "in `output.html.git-repository-url`",
                        RepoSource::Remote(..) => "from git remote \"origin\"",
                    })
                    .context("help: use `repo-url-template` option for a custom remote")
                    .context("failed to parse git remote url")?;
                GitHubPermalink::new(&owner, &repo, &reference).pipe(Permalink::GitHub)
            }
        };

        Ok(Ok(Self { root, link }))
    }
}

pub trait PermalinkFormat {
    /// Try to convert this path to a permalink
    fn to_link(&self, path: &str) -> Result<Url>;
    /// Try to extract a path (relative to repo root) from this link
    fn to_path(&self, link: &Url) -> Option<String>;
}

pub enum Permalink {
    GitHub(GitHubPermalink),
    Custom(CustomPermalink),
}

impl PermalinkFormat for Permalink {
    fn to_link(&self, path: &str) -> Result<Url> {
        match self {
            Self::GitHub(this) => this.to_link(path),
            Self::Custom(this) => this.to_link(path),
        }
    }

    fn to_path(&self, link: &Url) -> Option<String> {
        match self {
            Self::GitHub(this) => this.to_path(link),
            Self::Custom(this) => this.to_path(link),
        }
    }
}

pub struct GitHubPermalink(CustomPermalink);

impl PermalinkFormat for GitHubPermalink {
    fn to_link(&self, path: &str) -> Result<Url> {
        self.0.to_link(path)
    }

    fn to_path(&self, link: &Url) -> Option<String> {
        self.0.to_path(link)
    }
}

impl GitHubPermalink {
    /// c.f. <https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content--parameters>
    pub fn new(owner: &str, repo: &str, reference: &str) -> Self {
        let pattern = format!("https://github.com/{owner}/{repo}/{{tree}}/{{ref}}/{{path}}")
            .parse()
            .expect("should be a valid url");
        let reference = reference.into();
        Self(CustomPermalink { pattern, reference })
    }
}

pub struct CustomPermalink {
    pub pattern: Url,
    pub reference: String,
}

/// `{` and `}` are always percent-encoded in path [^1].
///
/// Encoding characters are always in uppercase [^2].
///
/// [^1]: <https://url.spec.whatwg.org/#path-percent-encode-set>
/// [^2]: <https://url.spec.whatwg.org/#percent-encode>
macro_rules! encoded_param {
    ($param:literal) => {
        concat!("%7B", $param, "%7D")
    };
}

impl PermalinkFormat for CustomPermalink {
    fn to_link(&self, path: &str) -> Result<Url> {
        let path = self
            .pattern
            .path()
            .split('/')
            .map(|segment| match segment {
                encoded_param!("ref") => &self.reference,
                encoded_param!("tree") => "tree",
                encoded_param!("path") => path,
                _ => segment,
            })
            .collect::<Vec<_>>()
            .join("/");

        let query = self
            .pattern
            .query_pairs()
            .fold(SearchParams::new(String::new()), |mut search, (k, v)| {
                match v.as_ref() {
                    "{ref}" => search.append_pair(&k, &self.reference),
                    "{tree}" => search.append_pair(&k, "tree"),
                    "{path}" => search.append_pair(&k, &path),
                    _ => search.append_pair(&k, &v),
                };
                search
            })
            .finish()
            .pipe(|query| if query.is_empty() { None } else { Some(query) });

        let fragment = self.pattern.fragment();

        self.pattern
            .clone()
            .tap_mut(|u| u.set_path(&path))
            .tap_mut(|u| u.set_query(query.as_deref()))
            .tap_mut(|u| u.set_fragment(fragment))
            .pipe(Ok)
    }

    // this is kind of ugly
    fn to_path(&self, link: &Url) -> Option<String> {
        if self.pattern.origin() != link.origin() {
            return None;
        }

        fn match_param(lhs: &str, rhs: Option<&str>) -> ControlFlow<()> {
            match lhs {
                encoded_param!("tree") => match rhs {
                    Some("tree" | "blob" | "raw") => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
                encoded_param!("ref") => match rhs {
                    Some("HEAD") => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
                lhs => match rhs {
                    Some(rhs) if lhs == rhs => ControlFlow::Continue(()),
                    _ => ControlFlow::Break(()),
                },
            }
        }

        let mut path = false;

        let mut lhs = self.pattern.path().split('/');
        let mut rhs = link.path().split('/');

        #[allow(clippy::while_let_on_iterator, reason = "symmetry")]
        while let Some(lhs) = lhs.next() {
            match lhs {
                encoded_param!("path") => {
                    path = true;
                    break;
                }
                lhs => match match_param(lhs, rhs.next()) {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(()) => return None,
                },
            }
        }

        while let Some(lhs) = lhs.next_back() {
            match match_param(lhs, rhs.next_back()) {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return None,
            }
        }

        let mut path = if path {
            Some(rhs.collect::<Vec<_>>().join("/"))
        } else {
            None
        };

        let link_query = link.query_pairs().collect::<HashMap<_, _>>();

        for (k, v) in self.pattern.query_pairs() {
            match v.as_ref() {
                "{path}" => match link_query.get(&k) {
                    Some(v) => {
                        path = if let Some(v) = v.strip_prefix('/') {
                            Some(v.into())
                        } else {
                            Some(v.as_ref().into())
                        }
                    }
                    None => return None,
                },
                "{tree}" => match link_query.get(&k).map(|v| &**v) {
                    Some("tree" | "blob" | "raw") => {}
                    _ => return None,
                },
                "{ref}" => match link_query.get(&k).map(|v| &**v) {
                    Some("HEAD") => {}
                    _ => return None,
                },
                _ => {}
            }
        }

        if let Some(path) = path {
            match link.fragment() {
                Some(fragment) => Some(format!("{path}#{fragment}")),
                None => Some(path),
            }
        } else {
            None
        }
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

fn get_git_head(repo: &Repository) -> Result<Option<String>> {
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
        log::info!("using tag {tag:?}");
        Ok(Some(tag))
    } else {
        let sha = head.id().to_string();
        log::info!("using commit {sha}");
        Ok(Some(sha))
    }
}

fn find_git_remote(repo: &Repository, config: &MDBookConfig) -> Result<Result<RepoSource>> {
    if let Some(url) = config
        .get::<String>("output.html.git-repository-url")
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
    use mdbook_preprocessor::config::Config as MDBookConfig;

    use super::{CustomPermalink, PermalinkFormat, find_git_remote, remote_as_github};

    #[test]
    fn test_github_url_from_book() -> Result<()> {
        let config = r#"
        [output.html]
        git-repository-url = "https://github.com/lorem/ipsum/tree/main/crates/dolor"
        "#
        .parse::<MDBookConfig>()?;
        let repo = Repository::open_from_env()?;
        let repo = find_git_remote(&repo, &config)??;
        let (owner, repo) = remote_as_github(repo.as_ref())?;
        assert_eq!(owner, "lorem");
        assert_eq!(repo, "ipsum");
        Ok(())
    }

    #[test]
    fn test_github_url_from_repo() -> Result<()> {
        let config = "".parse::<MDBookConfig>()?;
        let repo = Repository::open_from_env()?;
        let repo = find_git_remote(&repo, &config)??;
        let (_, repo) = remote_as_github(repo.as_ref())?;
        assert_eq!(repo, "mdbookkit");
        Ok(())
    }

    #[test]
    fn test_scp_uri() -> Result<()> {
        let config = r#"
        [output.html]
        git-repository-url = "git@my-alt.github.com:lorem/ipsum.git"
        "#
        .parse::<MDBookConfig>()?;
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
        .parse::<MDBookConfig>()
        .unwrap();
        let repo = Repository::open_from_env().unwrap();
        let repo = find_git_remote(&repo, &config).unwrap().unwrap();
        let _ = remote_as_github(repo.as_ref()).unwrap();
    }

    #[test]
    fn test_path_to_link() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let link = scheme.to_link(".editorconfig")?;

        assert_eq!(
            link.as_str(),
            "https://github.com/lorem/ipsum/tree/main/.editorconfig"
        );

        Ok(())
    }

    #[test]
    fn test_path_to_link_with_suffix() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "master".into(),
        };

        let link = scheme.to_link(".editorconfig")?;

        assert_eq!(
            link.as_str(),
            "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=master"
        );

        Ok(())
    }

    #[test]
    fn test_link_to_path() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD/path/to/file".parse()?);

        assert_eq!(path.as_deref(), Some("path/to/file"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_with_fragments() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(
            &"https://github.com/lorem/ipsum/blob/HEAD/path/to/file.md#section-1".parse()?,
        );

        assert_eq!(path.as_deref(), Some("path/to/file.md#section-1"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_repo_root() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://github.com/lorem/ipsum/{tree}/{ref}/{path}".parse()?,
            reference: "main".into(),
        };

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD".parse()?);

        assert_eq!(path.as_deref(), Some(""));

        let path = scheme.to_path(&"https://github.com/lorem/ipsum/raw/HEAD/".parse()?);

        assert_eq!(path.as_deref(), Some(""));

        Ok(())
    }

    #[test]
    fn test_link_to_path_with_suffix() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let path =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=HEAD".parse()?);

        assert_eq!(path.as_deref(), Some(".editorconfig"));

        Ok(())
    }

    #[test]
    fn test_link_to_path_non_head() -> Result<()> {
        let scheme = CustomPermalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let matched =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=b676ac4".parse()?);

        assert!(matched.is_none());

        Ok(())
    }
}
