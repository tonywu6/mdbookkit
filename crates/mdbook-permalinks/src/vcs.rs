use anyhow::{Context, Result, anyhow, bail};
use git2::{DescribeOptions, Repository, RepositoryOpenFlags};
use mdbook_preprocessor::{PreprocessorContext, config::Config as MDBookConfig};
use tap::Pipe;
use tracing::{debug, info, instrument, trace};
use url::Url;

use mdbookkit::{
    emit_debug,
    url::{UrlFromPath, UrlPath},
};

use crate::{
    Config, VersionControl,
    link::{ContentHint, PathStatus},
};

impl VersionControl {
    #[instrument(level = "debug", skip_all)]
    pub fn try_from_git(config: &Config, ctx: &PreprocessorContext) -> Result<Result<Self>> {
        let repo = match Repository::open_ext(
            &ctx.root,
            RepositoryOpenFlags::empty(),
            &[] as &[&std::ffi::OsStr],
        )
        .context("this preprocessor requires a git repository to work")
        {
            Ok(repo) => repo,
            Err(err) => return config.fail_on_warnings.adjusted(Ok(Err(err))),
        };

        let root = repo
            .workdir()
            .unwrap_or_else(|| repo.commondir())
            .canonicalize()
            .context("could not locate repo root")?
            .to_directory_url();

        let Some(reference) =
            get_git_head(&repo).context("could not get a tag or the commit hash to HEAD")?
        else {
            let err = anyhow!("repo does not contain any commit")
                .context("this preprocessor expects repo to have at least 1 commit");
            return config.fail_on_warnings.adjusted(Ok(Err(err)));
        };

        let link = {
            if let Some(pat) = &config.repo_url_template {
                debug!("using explicitly set repo_url_template");
                let pattern = match pat.parse::<UrlPath>() {
                    Ok(pat) => {
                        if pat.is_url() {
                            Ok(pat)
                        } else {
                            Err(anyhow!("URL must begin with `https://` or `http://`"))
                        }
                    }
                    Err(e) => Err(anyhow!(e)),
                }
                .context("failed to parse `repo-url-template` as a valid URL")?;
                Permalink { pattern, reference }
            } else {
                let repo = match find_git_remote(&repo, &ctx.config)
                    .context("error while finding a git remote URL")?
                {
                    Ok(repo) => repo,
                    Err(err) => {
                        return anyhow! { "help: set `output.html.git-repository-url` to a GitHub URL, \
                                        or use `repo-url-template` option" }
                            .context(err)
                            .context("failed to determine the remote URL prefix for permalinks")
                            .pipe(Err)
                            .pipe(Ok)
                            .pipe(|result| config.fail_on_warnings.adjusted(result));
                    }
                };
                let (owner, repo) = match remote_as_github(repo.as_ref()) {
                    Ok(result) => result,
                    Err(err) => {
                        return anyhow! { "help: use the `repo-url-template` option \
                        to define a custom URL scheme" }
                        .context(err)
                        .context(match repo {
                            RepoSource::Config(..) => "in `output.html.git-repository-url`:",
                            RepoSource::Remote(..) => "in git remote \"origin\":",
                        })
                        .context("failed to find a git remote URL")
                        .pipe(Err);
                    }
                };
                Permalink::github(&owner, &repo, &reference)
            }
        };

        Ok(Ok(Self { root, repo, link }))
    }

    #[instrument(level="debug", skip_all, fields(file = format!("{file}"), root = format!("{}", self.root)))]
    pub fn try_file(&self, file: &Url) -> Result<TryFile, PathStatus> {
        let Some(path) = self.root.make_relative(file) else {
            debug!("no relative path from root");
            return Err(PathStatus::Unreachable);
        };

        if path.starts_with("../") {
            debug!("path outside repo");
            return Err(PathStatus::NotInRepo);
        }

        if let Ok(metadata) = file
            .to_file_path()
            .expect("should be a file: url")
            .symlink_metadata()
        {
            if !(self.repo.is_path_ignored(&path))
                .with_context(|| format!("error testing if {path:?} is ignored"))
                .or_else(emit_debug!())
                .unwrap_or(false)
            {
                Ok(TryFile { path, metadata }).inspect(|f| trace!("{f:?}"))
            } else {
                debug!("path ignored");
                Err(PathStatus::Ignored)
            }
        } else {
            debug!("path inaccessible");
            Err(PathStatus::Unreachable)
        }
    }
}

#[derive(Debug)]
pub struct TryFile {
    pub path: String,
    pub metadata: std::fs::Metadata,
}

#[derive(Debug)]
pub struct Permalink {
    pub pattern: UrlPath,
    pub reference: String,
}

impl Permalink {
    /// See <https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content--parameters>
    pub fn github(owner: &str, repo: &str, reference: &str) -> Self {
        let pattern = format!("https://github.com/{owner}/{repo}/{{tree}}/{{ref}}/{{path}}")
            .parse()
            .expect("should be a valid pattern");
        let reference = reference.into();
        Self { pattern, reference }
    }
}

impl Permalink {
    /// Try to convert this path to a permalink
    pub fn to_link(&self, path: &str, hint: ContentHint) -> Url {
        self.pattern
            .fill_pattern(|group| match group {
                "ref" => Some((&self.reference).into()),
                "tree" => Some(
                    match hint {
                        ContentHint::Tree => "tree",
                        ContentHint::Raw => "raw",
                    }
                    .into(),
                ),
                "path" => Some(path.into()),
                _ => None,
            })
            .into_url()
            .expect("should result in a URL")
    }

    /// Try to extract a path (relative to repo root) from this link
    pub fn to_path(&self, link: &Url) -> Option<(String, ContentHint)> {
        let mut groups = self.pattern.test_pattern(Some("path"), link)?;

        if groups.get("ref").map(|s| &**s) != Some("HEAD") {
            return None;
        }

        let hint = match groups.get("tree").map(|s| &**s)? {
            "tree" | "blob" => ContentHint::Tree,
            "raw" => ContentHint::Raw,
            _ => return None,
        };

        let path = groups.remove("path")?.into_owned();

        debug!(?path, ?hint, "path matched");

        Some((path, hint))
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

#[instrument(level = "debug", skip_all)]
fn get_git_head(repo: &Repository) -> Result<Option<String>> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(err) => {
            debug!("could not resolve the currently checked-out ref: {err}");
            return Ok(None);
        }
    };

    let head = head
        .peel_to_commit()
        .context("failed to resolve the commit HEAD is at")?;

    debug!("HEAD is at {}", head.id());

    if let Ok(tag) = head
        .as_object()
        .describe(
            DescribeOptions::new()
                .describe_tags()
                .max_candidates_tags(0), // exact match
        )
        .and_then(|tag| tag.format(None))
        .or_else(emit_debug!("no exact tag found: {}"))
    {
        info!("using tag name {tag:?} for permalinks");
        Ok(Some(tag))
    } else {
        let sha = head.id().to_string();
        info!("using commit hash {sha} for permalinks");
        Ok(Some(sha))
    }
}

#[instrument(level = "debug", skip_all)]
fn find_git_remote(repo: &Repository, config: &MDBookConfig) -> Result<Result<RepoSource>> {
    if let Some(url) = config.get::<String>("output.html.git-repository-url")? {
        debug!("found {url:?} in book.toml");
        gix_url::parse(url.as_str().into())
            .inspect(|u| debug!("parsed as {u:?}"))
            .context("could not parse `output.html.git-repository-url`")?
            .pipe(RepoSource::Config)
            .pipe(Ok)
            .pipe(Ok)
    } else {
        let repo = match repo
            .find_remote("origin")
            .context("expected repo to have a remote named `origin`, but found none")
        {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };
        let repo = match repo.url() {
            Some(url) => url,
            None => {
                return anyhow!("expected remote `origin` to have a URL, but found none")
                    .pipe(Err)
                    .pipe(Ok);
            }
        };
        debug!("found {repo:?} via remote `origin`");
        gix_url::parse(repo.into())
            .inspect(|u| debug!("parsed as {u:?}"))
            .context("could not parse the remote URL of `origin`")?
            .pipe(RepoSource::Remote)
            .pipe(Ok)
            .pipe(Ok)
    }
}

fn remote_as_github(url: &gix_url::Url) -> Result<(String, String)> {
    let Some(host) = url.host() else {
        bail!("remote URL does not have a host")
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
#[allow(clippy::unwrap_used)]
mod tests {
    use anyhow::Result;
    use git2::Repository;
    use mdbook_preprocessor::config::Config as MDBookConfig;

    use crate::link::ContentHint;

    use super::{Permalink, find_git_remote, remote_as_github};

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
        let scheme = Permalink::github("lorem", "ipsum", "main");

        let link = scheme.to_link(".editorconfig", ContentHint::Tree);

        assert_eq!(
            link.as_str(),
            "https://github.com/lorem/ipsum/tree/main/.editorconfig"
        );

        Ok(())
    }

    #[test]
    fn test_path_to_link_with_suffix() -> Result<()> {
        let scheme = Permalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "master".into(),
        };

        let link = scheme.to_link(".editorconfig", ContentHint::Tree);

        assert_eq!(
            link.as_str(),
            "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=master"
        );

        Ok(())
    }

    #[test]
    fn test_link_to_path() -> Result<()> {
        let scheme = Permalink::github("lorem", "ipsum", "main");

        let (path, hint) = scheme
            .to_path(&"https://github.com/lorem/ipsum/raw/HEAD/path/to/file".parse()?)
            .unwrap();

        assert_eq!(path, "path/to/file");
        assert_eq!(hint, ContentHint::Raw);

        Ok(())
    }

    #[test]
    fn test_link_to_path_repo_root() -> Result<()> {
        let scheme = Permalink::github("lorem", "ipsum", "main");

        let (path, _) = scheme
            .to_path(&"https://github.com/lorem/ipsum/raw/HEAD".parse()?)
            .unwrap();

        assert_eq!(path, "");

        let (path, _) = scheme
            .to_path(&"https://github.com/lorem/ipsum/raw/HEAD/".parse()?)
            .unwrap();

        assert_eq!(path, "");

        Ok(())
    }

    #[test]
    fn test_link_to_path_with_suffix() -> Result<()> {
        let scheme = Permalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let (path, hint) =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/raw/.editorconfig?h=HEAD".parse()?).unwrap();

        assert_eq!(path, ".editorconfig");
        assert_eq!(hint, ContentHint::Raw);

        Ok(())
    }

    #[test]
    fn test_link_to_path_non_head() -> Result<()> {
        let scheme = Permalink {
            pattern: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let matched =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=b676ac4".parse()?);

        assert!(matched.is_none());

        Ok(())
    }
}
