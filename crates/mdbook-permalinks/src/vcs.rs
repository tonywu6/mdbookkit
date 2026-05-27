use anyhow::{Context, Result, anyhow, bail};
use git2::{DescribeOptions, Repository, RepositoryOpenFlags};
use mdbook_preprocessor::{PreprocessorContext, config::Config as MDBookConfig};
use tap::Pipe;
use tracing::{debug, info, instrument, trace, warn};
use url::Url;

use mdbookkit::{
    emit_debug,
    error::WithPathDebug,
    url::{UrlFromPath, UrlUtil},
};

use crate::{
    VersionControl,
    link::{ContentHint, PathStatus},
    options::{Config, PathParams, TemplateConfig},
};

impl VersionControl {
    #[instrument(level="debug", skip_all, fields(
        file = ?file.debug(),
        root = ?self.root.debug(),
    ))]
    pub fn try_file(&self, file: &Url) -> Result<TryFile, PathStatus> {
        let Some(link) = self.root.make_relative(file) else {
            debug!("no relative path from root");
            return Err(PathStatus::Unreachable);
        };

        if link.starts_with("../") {
            debug!("path outside repo");
            return Err(PathStatus::NotInRepo);
        };

        let path = file.expect_path();

        match path.symlink_metadata() {
            Ok(metadata) => {
                if !(self.repo.is_path_ignored(&path))
                    .with_path_debug(&path)
                    .context("error testing if path is ignored")
                    .or_else(emit_debug!())
                    .unwrap_or(false)
                {
                    Ok(TryFile { link, metadata }).inspect(|f| trace!("{f:?}"))
                } else {
                    debug!("path ignored");
                    Err(PathStatus::Ignored)
                }
            }
            Err(error) => match error.kind() {
                std::io::ErrorKind::NotFound => {
                    debug!("path not found");
                    Err(PathStatus::NotFound)
                }
                std::io::ErrorKind::NotADirectory => {
                    debug!("path not a directory");
                    Err(PathStatus::NotADirectory)
                }
                _ => {
                    debug!("path inaccessible");
                    Err(PathStatus::Unreachable)
                }
            },
        }
    }
}

#[derive(Debug)]
pub struct TryFile {
    pub link: String,
    pub metadata: std::fs::Metadata,
}

#[derive(Debug)]
pub struct Permalink {
    pattern: Url,
    refname: RefName,
    params: PathParams,
}

#[derive(Debug)]
enum RefName {
    Commit(String),
    Tag(String),
}

impl Permalink {
    /// Try to convert this path to a permalink
    pub fn to_link(&self, path: &str, hint: ContentHint) -> Url {
        self.pattern.pattern_fill(|group| match group {
            "ref" => Some(
                match &self.refname {
                    RefName::Commit(commit) => commit,
                    RefName::Tag(tag) => tag,
                }
                .into(),
            ),
            "kind" => Some(
                match &self.refname {
                    RefName::Commit(..) => &self.params.commit[0],
                    RefName::Tag(..) => &self.params.tag[0],
                }
                .into(),
            ),
            "tree" => Some(
                match hint {
                    ContentHint::Tree => &self.params.tree[0],
                    ContentHint::Raw => &self.params.raw[0],
                }
                .into(),
            ),
            "path" => Some(path.into()),
            _ => None,
        })
    }

    /// Try to extract a path (relative to repo root) from this link
    pub fn to_path(&self, link: &Url) -> Option<(String, ContentHint)> {
        let mut groups = self.pattern.pattern_test(Some("path"), link)?;

        if groups.get("ref").map(|s| &**s) != Some("HEAD") {
            return None;
        }

        let hint = if let Some(tree) = groups.get("tree").map(|s| &**s) {
            if self.params.tree.iter().any(|plc| plc == tree) {
                ContentHint::Tree
            } else if self.params.raw.iter().any(|plc| plc == tree) {
                ContentHint::Raw
            } else {
                return None;
            }
        } else {
            return None;
        };

        let path = groups.remove("path")?.into_owned();

        debug!(?path, ?hint, "path matched");

        Some((path, hint))
    }
}

impl AsRef<Url> for Permalink {
    fn as_ref(&self) -> &Url {
        &self.pattern
    }
}

impl Default for PathParams {
    fn default() -> Self {
        Self {
            tree: vec!["tree".into(), "blob".into()],
            raw: vec!["raw".into()],
            commit: vec!["commit".into()],
            tag: vec!["tag".into()],
        }
    }
}

enum RepoSource {
    Config(gix_url::Url),
    Remote(gix_url::Url),
}

impl RepoSource {
    fn as_url(&self) -> &gix_url::Url {
        match self {
            Self::Config(u) => u,
            Self::Remote(u) => u,
        }
    }
}

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

        let root = repo.workdir().unwrap_or_else(|| repo.commondir());
        let root = root
            .canonicalize()
            .with_path_debug(root)
            .context("could not locate repo root")?
            .dir_to_url();

        let refname = match get_git_head(&repo)
            .context("could not get a tag or the commit hash to HEAD")?
        {
            Some(refname) => refname,
            None => {
                warn! { "the current repo does not have any commit" }
                warn! { "URLs generated by the preprocessor will fallback to using `HEAD` as the reference" };
                RefName::Commit("HEAD".into())
            }
        };

        let link = {
            let TemplateConfig { pattern, params } = &config.repo_url_template;

            let pattern = if let Some(pattern) = pattern {
                debug!("using explicitly set repo_url_template");
                pattern.clone()
            } else {
                let remote = config.remote_name.as_deref().unwrap_or("origin");
                let repo = match find_git_remote(&repo, remote, &ctx.config)
                    .context("error while finding a git remote URL")?
                {
                    Ok(repo) => repo,
                    Err(err) => {
                        return anyhow! { "help: set `output.html.git-repository-url` to a \
                        supported URL, or use `repo-url-template` option" }
                        .context(err)
                        .context("failed to determine the remote URL prefix for permalinks")
                        .pipe(Err)
                        .pipe(Ok)
                        .pipe(|result| config.fail_on_warnings.adjusted(result));
                    }
                };

                match derive_pattern(repo.as_url()) {
                    Ok(pattern) => pattern,
                    Err(err) => {
                        return anyhow! { "help: use the `repo-url-template` option \
                        to define a custom URL scheme" }
                        .context(err)
                        .context(format!("{:?}", repo.as_url().to_string()))
                        .context(match repo {
                            RepoSource::Config(..) => "in `output.html.git-repository-url`:".into(),
                            RepoSource::Remote(..) => format!("in git remote {remote:?}:"),
                        })
                        .context("could not find a supported git remote URL")
                        .pipe(Err);
                    }
                }
            };

            let params = match params {
                Some(params) => params.clone(),
                None => derive_params(&pattern),
            };

            Permalink {
                pattern,
                refname,
                params,
            }
        };

        debug!("{link:#?}");

        Ok(Ok(Self { root, repo, link }))
    }
}

#[instrument(level = "debug", skip_all)]
fn get_git_head(repo: &Repository) -> Result<Option<RefName>> {
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
        Ok(Some(RefName::Tag(tag)))
    } else {
        let sha = head.id().to_string();
        info!("using commit hash {sha} for permalinks");
        Ok(Some(RefName::Commit(sha)))
    }
}

#[instrument(level = "debug", skip_all)]
fn find_git_remote(
    repo: &Repository,
    remote: &str,
    config: &MDBookConfig,
) -> Result<Result<RepoSource>> {
    if let Some(url) = config.get::<String>("output.html.git-repository-url")? {
        debug!("found {url:?} in book.toml");
        gix_url::parse(url.as_str().into())
            .inspect(|u| debug!("parsed as {u:?}"))
            .context("could not parse `output.html.git-repository-url`")?
            .pipe(RepoSource::Config)
            .pipe(Ok)
            .pipe(Ok)
    } else {
        let repo = match repo.find_remote(remote).with_context(|| {
            format!("expected repo to have a remote named {remote:?}, but found none")
        }) {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };
        let repo = match repo.url() {
            Some(url) => url,
            None => {
                return anyhow!("expected remote {remote:?} to have a URL, but found none")
                    .pipe(Err)
                    .pipe(Ok);
            }
        };
        debug!("found {repo:?} via remote {remote:?}");
        gix_url::parse(repo.into())
            .inspect(|u| debug!("parsed as {u:?}"))
            .with_context(|| format!("could not parse the remote URL of {remote:?}"))?
            .pipe(RepoSource::Remote)
            .pipe(Ok)
            .pipe(Ok)
    }
}

fn derive_pattern(url: &gix_url::Url) -> Result<Url> {
    let host = match url.host() {
        Some(host) => host,
        None => bail!("remote URL does not have a host"),
    };
    let path = url.path.to_string();

    fn is_on_domain(domain: &'static str, host: &str) -> bool {
        match host.strip_suffix(domain) {
            None => false,
            Some(sub) => sub.is_empty() || sub.ends_with('.'),
        }
    }

    if is_on_domain("github.com", host) {
        let malformed = || {
            format! { "malformed path {path:?}: expected URL for {host:?} \
            to begin with `/<owner>/<repo>`" }
        };

        let mut iter = path.split('/').skip_while(|c| c.is_empty());
        let owner = (iter.next()).with_context(malformed)?;
        let repo = (iter.next()).with_context(malformed)?;
        let repo = repo.strip_suffix(".git").unwrap_or(repo);

        return derive_pattern_github(owner, repo);
    }

    if is_on_domain("codeberg.org", host) {
        let malformed = || {
            format! { "malformed path {path:?}: expected URL for {host:?} \
            to begin with `/<owner>/<repo>`" }
        };

        let mut iter = path.split('/').skip_while(|c| c.is_empty());
        let owner = (iter.next()).with_context(malformed)?;
        let repo = (iter.next()).with_context(malformed)?;
        let repo = repo.strip_suffix(".git").unwrap_or(repo);

        return derive_pattern_codeberg(owner, repo);
    }

    if is_on_domain("tangled.org", host) {
        let malformed = || {
            format! { "malformed path {path:?}: expected URL for {host:?} \
            to begin with `/<owner>/<repo>` or `/<did>`" }
        };

        let mut iter = path.split('/').skip_while(|c| c.is_empty());
        let entity = (iter.next()).with_context(malformed)?;
        let repo = if entity.starts_with("did:") {
            None
        } else {
            let repo = (iter.next()).with_context(malformed)?;
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            Some(repo)
        };

        return derive_pattern_tangled(entity, repo);
    }

    if host.starts_with("knot.") {
        warn! { "help: it looks like you are using a self-hosted tangled knot" };
        warn! { "help: if so, you can set `output.html.git-repository-url` \
        to your repo's \"https://tangled.org\" URL" }
    }

    bail!("unsupported remote {host:?}")
}

fn derive_pattern_github(owner: &str, repo: &str) -> Result<Url> {
    let pattern = format!("https://github.com/{owner}/{repo}/{{tree}}/{{ref}}/{{path}}");
    let pattern = pattern
        .parse()
        .with_context(|| format!("could not parse {pattern:?} as a URL"))?;
    Ok(pattern)
}

fn derive_pattern_codeberg(owner: &str, repo: &str) -> Result<Url> {
    let pattern = format!("https://codeberg.org/{owner}/{repo}/{{tree}}/{{kind}}/{{ref}}/{{path}}");
    let pattern = pattern
        .parse()
        .with_context(|| format!("could not parse {pattern:?} as a URL"))?;
    Ok(pattern)
}

fn derive_pattern_tangled(entity: &str, repo: Option<&str>) -> Result<Url> {
    let pattern = match repo {
        Some(repo) => format!("https://tangled.org/{entity}/{repo}/{{tree}}/{{ref}}/{{path}}"),
        None => format!("https://tangled.org/{entity}/{{tree}}/{{ref}}/{{path}}"),
    };
    let pattern = pattern
        .parse()
        .with_context(|| format!("could not parse {pattern:?} as a URL"))?;
    Ok(pattern)
}

fn derive_params(pat: &Url) -> PathParams {
    match pat.host_str() {
        Some("github.com") => Default::default(),
        Some("tangled.org") => Default::default(),
        Some("codeberg.org") => PathParams {
            tree: vec!["src".into()],
            ..Default::default()
        },
        _ => Default::default(),
    }
}
