use std::{collections::HashMap, ops::ControlFlow};

use anyhow::{Context, Result, anyhow, bail};
use git2::{DescribeOptions, Repository};
use mdbook_preprocessor::config::Config as MDBookConfig;
use tap::{Pipe, Tap};
use tracing::{debug, info, instrument, trace};
use url::{Url, form_urlencoded::Serializer as SearchParams};

use mdbookkit::{emit_debug, emit_trace, url::UrlFromPath};

use crate::{
    Config, VersionControl,
    link::{ContentHint, PathStatus},
};

impl VersionControl {
    #[instrument(level = "debug", skip_all)]
    pub fn try_from_git(config: &Config, book: &MDBookConfig) -> Result<Result<Self>> {
        let repo = match Repository::open_from_env()
            .context("Preprocessor requires a git repository to work")
            .context("Could not find a git repository")
        {
            Ok(repo) => repo,
            Err(err) => return config.fail_on_warnings.adjusted(Ok(Err(err))),
        };

        let root = repo
            .workdir()
            .unwrap_or_else(|| repo.commondir())
            .canonicalize()
            .context("Could not locate repo root")?
            .to_directory_url();

        let Some(reference) =
            get_git_head(&repo).context("Could not get a tag or the commit hash to HEAD")?
        else {
            return config
                .fail_on_warnings
                .adjusted(Ok(Err(anyhow!("No commit found in this repo"))));
        };

        let link = {
            if let Some(pat) = &config.repo_url_template {
                debug!("using explicitly set repo_url_template");
                Permalink {
                    template: (pat.parse())
                        .context("Failed to parse `repo-url-template` as a valid URL")?,
                    reference,
                }
            } else {
                let repo = match find_git_remote(&repo, book)
                    .context("Error while finding a git remote URL")?
                {
                    Ok(repo) => repo,
                    Err(err) => {
                        return anyhow!("help: or use `repo-url-template` option")
                            .context("help: set `output.html.git-repository-url` to a GitHub URL")
                            .context(err)
                            .context("Failed to determine the remote URL prefix for permalinks")
                            .pipe(Err)
                            .pipe(Ok)
                            .pipe(|result| config.fail_on_warnings.adjusted(result));
                    }
                };
                let (owner, repo) = match remote_as_github(repo.as_ref()) {
                    Ok(result) => result,
                    Err(err) => {
                        return anyhow! {"help: use the `repo-url-template` option \
                        to define a custom URL scheme"}
                        .context(err)
                        .context(match repo {
                            RepoSource::Config(..) => "In `output.html.git-repository-url`:",
                            RepoSource::Remote(..) => "In git remote \"origin\":",
                        })
                        .context("Failed to find a git remote URL")
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
            if !self.repo.is_path_ignored(&path).unwrap_or(false) {
                Ok(TryFile { path, metadata }).inspect(emit_trace!())
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

pub trait PermalinkFormat {
    /// Try to convert this path to a permalink
    fn to_link(&self, path: &str, hint: ContentHint) -> Result<Url>;
    /// Try to extract a path (relative to repo root) from this link
    fn to_path(&self, link: &Url) -> Option<(String, ContentHint)>;
}

#[derive(Debug)]
pub struct Permalink {
    pub template: Url,
    pub reference: String,
}

impl Permalink {
    /// See <https://docs.github.com/en/rest/repos/contents?apiVersion=2022-11-28#get-repository-content--parameters>
    pub fn github(owner: &str, repo: &str, reference: &str) -> Self {
        let template = format!("https://github.com/{owner}/{repo}/{{tree}}/{{ref}}/{{path}}")
            .parse()
            .expect("should be a valid url");
        let reference = reference.into();
        Self {
            template,
            reference,
        }
    }
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

impl PermalinkFormat for Permalink {
    fn to_link(&self, path: &str, hint: ContentHint) -> Result<Url> {
        let path = self
            .template
            .path()
            .split('/')
            .map(|segment| match segment {
                encoded_param!("ref") => &self.reference,
                encoded_param!("tree") => match hint {
                    ContentHint::Tree => "tree",
                    ContentHint::Raw => "raw",
                },
                encoded_param!("path") => path,
                _ => segment,
            })
            .collect::<Vec<_>>()
            .join("/");

        let query = self
            .template
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

        let fragment = self.template.fragment();

        self.template
            .clone()
            .tap_mut(|u| u.set_path(&path))
            .tap_mut(|u| u.set_query(query.as_deref()))
            .tap_mut(|u| u.set_fragment(fragment))
            .pipe(Ok)
    }

    // this is kind of messy
    #[instrument("url_to_path", level = "debug", skip_all, fields(url = format!("{link}")))]
    fn to_path(&self, link: &Url) -> Option<(String, ContentHint)> {
        if self.template.origin() != link.origin() {
            return None;
        }

        let mut path = false;
        let mut hint = ContentHint::Tree;

        let mut match_param = |lhs: &str, rhs: Option<&str>| -> ControlFlow<()> {
            trace!("match param {lhs:?} .. {rhs:?}");
            match lhs {
                encoded_param!("tree") => match rhs {
                    Some("tree" | "blob") => {
                        hint = ContentHint::Tree;
                        ControlFlow::Continue(())
                    }
                    Some("raw") => {
                        hint = ContentHint::Raw;
                        ControlFlow::Continue(())
                    }
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
        };

        let mut lhs = self.template.path().split('/');
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
                    ControlFlow::Break(()) => {
                        trace!("no {{path}} found");
                        return None;
                    }
                },
            }
        }

        while let Some(lhs) = lhs.next_back() {
            match match_param(lhs, rhs.next_back()) {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => {
                    trace!("insufficient {{path}}");
                    return None;
                }
            }
        }

        let mut path = if path {
            Some(rhs.collect::<Vec<_>>().join("/"))
        } else {
            None
        };

        let link_query = link.query_pairs().collect::<HashMap<_, _>>();

        for (k, v) in self.template.query_pairs() {
            trace!("match query {k:?} .. {v:?}");
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
                    Some("tree" | "blob") => {
                        hint = ContentHint::Tree;
                    }
                    Some("raw") => {
                        hint = ContentHint::Raw;
                    }
                    _ => return None,
                },
                "{ref}" => match link_query.get(&k).map(|v| &**v) {
                    Some("HEAD") => {}
                    _ => return None,
                },
                _ => {}
            }
        }

        debug!(?path, ?hint, "path matched");

        Some((path?, hint))
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
        .context("Failed to resolve the commit HEAD is at")?;

    debug!("HEAD is at {}", head.id());

    if let Ok(tag) = head
        .as_object()
        .describe(
            DescribeOptions::new()
                .describe_tags()
                .max_candidates_tags(0), // exact match
        )
        .and_then(|tag| tag.format(None))
        .inspect_err(emit_debug!("no exact tag found: {}"))
    {
        info!("Using tag name {tag:?} for permalinks");
        Ok(Some(tag))
    } else {
        let sha = head.id().to_string();
        info!("Using commit hash {sha} for permalinks");
        Ok(Some(sha))
    }
}

#[instrument(level = "debug", skip_all)]
fn find_git_remote(repo: &Repository, config: &MDBookConfig) -> Result<Result<RepoSource>> {
    if let Some(url) = config.get::<String>("output.html.git-repository-url")? {
        debug!("found {url:?} in book.toml");
        gix_url::parse(url.as_str().into())
            .inspect(emit_debug!("parsed as {:?}"))?
            .pipe(RepoSource::Config)
            .pipe(Ok)
            .pipe(Ok)
    } else {
        let repo = match repo
            .find_remote("origin")
            .context("Repo does not have remote named `origin`")
        {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };
        let repo = match repo.url() {
            Some(url) => url,
            None => return Ok(Err(anyhow!("Remote `origin` does not have a URL"))),
        };
        debug!("found {repo:?} via remote `origin`");
        gix_url::parse(repo.into())
            .inspect(emit_debug!("parsed as {:?}"))?
            .pipe(RepoSource::Remote)
            .pipe(Ok)
            .pipe(Ok)
    }
}

fn remote_as_github(url: &gix_url::Url) -> Result<(String, String)> {
    let Some(host) = url.host() else {
        bail!("Remote URL does not have a host")
    };

    if host != "github.com" && !host.ends_with(".github.com") {
        bail!("Unsupported remote {host:?}, only `github.com` is supported")
    }

    let path = url.path.to_string();

    let mut iter = path.split('/').skip_while(|c| c.is_empty()).take(2);

    let owner = iter
        .next()
        .with_context(|| format!("Malformed path {path:?}, expected `/<owner>/<repo>`"))?;

    let repo = iter
        .next()
        .with_context(|| format!("Malformed path {path:?}, expected `/<owner>/<repo>`"))?;

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

    use super::{Permalink, PermalinkFormat, find_git_remote, remote_as_github};

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
    #[should_panic(expected = "Unsupported remote")]
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

        let link = scheme.to_link(".editorconfig", ContentHint::Tree)?;

        assert_eq!(
            link.as_str(),
            "https://github.com/lorem/ipsum/tree/main/.editorconfig"
        );

        Ok(())
    }

    #[test]
    fn test_path_to_link_with_suffix() -> Result<()> {
        let scheme = Permalink {
            template: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "master".into(),
        };

        let link = scheme.to_link(".editorconfig", ContentHint::Tree)?;

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
            template: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
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
            template: "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}".parse()?,
            reference: "main".into(),
        };

        let matched =
            scheme.to_path(&"https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/.editorconfig?h=b676ac4".parse()?);

        assert!(matched.is_none());

        Ok(())
    }
}
