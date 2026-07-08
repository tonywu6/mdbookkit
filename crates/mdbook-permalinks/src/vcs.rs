use std::{
    fmt::Debug,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use git2::{DescribeOptions, Repository, RepositoryOpenFlags};
use tap::Pipe;
use tracing::{debug, info, instrument, trace, warn};
use url::Url;

use mdbookkit::{
    doc_link, emit_debug, emit_warning,
    error::{Show, WithDebugContext},
    url::{RelativeUrl, UrlFromPath, UrlUtil},
};

use crate::{
    link::{ContentInterest, LinkError, PathError},
    options::{Config, PathParams, TemplateConfig},
};

pub struct VersionControl {
    root: Url,
    link: Permalink,
    repo: Repository,
}

impl VersionControl {
    #[instrument(
        level = "debug",
        "repo_try_file",
        skip_all,
        err(Debug, level = "debug")
    )]
    pub fn try_file(&self, url: Url) -> Result<TryRepoPath, LinkError> {
        let link = self.path_info(url, None)?;

        let real_path = match link.std_path.canonicalize() {
            Ok(path) => path,
            Err(err) => {
                trace!(error = ?err, "could not resolve path");
                return Err(PathError::from_io(err).at(link.url));
            }
        };

        if link.std_path == real_path {
            trace!("path is canonical");

            Ok(TryRepoPath::Canonical { link })
        } else {
            let url = if link.url.path().ends_with('/') {
                real_path.dir_to_url()
            } else {
                real_path.file_to_url()
            }
            .include_after_path(&link.relative);

            debug! {
                link_path = ?self.root().as_base().show_path(&link.url),
                real_path = ?self.root().as_base().show_path(&url),
                "path is not canonical"
            };

            let real = self.path_info(url, Some(real_path))?;
            Ok(TryRepoPath::Noncanonical { link, real })
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn path_info(&self, url: Url, path: Option<PathBuf>) -> Result<RepoPath, LinkError> {
        let relative = match self.root.as_base().make_relative_scoped(&url) {
            Some(href) => href,
            None => return Err(PathError::NotInRepo.at(url)),
        };

        trace!(relative = ?relative.show_path());

        let std_path = match path {
            Some(path) => path,
            None => match url.to_file_path() {
                Ok(path) => path,
                Err(()) => return Err(PathError::InvalidEncoding.at(url)),
            },
        };

        trace!(std_path = ?std_path.show());

        let is_dir = match std_path.symlink_metadata() {
            Ok(metadata) => metadata.is_dir(),
            Err(error) => {
                trace!(?error, "error reading metadata");
                return Err(PathError::from_io(error).at(url));
            }
        };

        trace!(?is_dir);

        let is_ignored = match self
            .repo
            .is_path_ignored(&std_path)
            .with_path_debug(&std_path)
            .context({
                "error while checking if this path is gitignored; \
                assuming it is not ignored"
            })
            .or_else(emit_warning!())
        {
            Ok(true) => GitIgnore::Ignored,
            Ok(false) => GitIgnore::NotIgnored,
            Err(()) => GitIgnore::NotIgnored,
        };

        trace!(?is_ignored);

        Ok(RepoPath {
            url,
            relative,
            std_path,
            is_dir,
            is_ignored,
        })
    }

    pub fn root(&self) -> &Url {
        &self.root
    }

    pub fn scheme(&self) -> &Permalink {
        &self.link
    }
}

#[derive(Debug)]
pub enum TryRepoPath {
    Canonical { link: RepoPath },
    Noncanonical { link: RepoPath, real: RepoPath },
}

pub struct RepoPath {
    pub url: Url,
    pub relative: RelativeUrl,
    pub std_path: PathBuf,
    pub is_ignored: GitIgnore,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum GitIgnore {
    Ignored,
    NotIgnored,
}

pub struct Permalink {
    pattern: Url,
    refname: RefName,
    params: PathParams,
}

#[derive(Debug)]
enum RefName {
    Commit(String),
    Tag(String),
    Head,
}

impl Permalink {
    /// Try to convert this relative url to a permalink
    pub fn to_link(&self, href: &RelativeUrl, interest: ContentInterest) -> Url {
        self.to_link_with_ref(href, interest, &self.refname)
    }

    /// Try to convert this relative url to a permalink
    pub fn to_link_at_head(&self, href: &RelativeUrl, interest: ContentInterest) -> Url {
        self.to_link_with_ref(href, interest, &RefName::Head)
    }

    #[inline]
    fn to_link_with_ref(
        &self,
        href: &RelativeUrl,
        interest: ContentInterest,
        refname: &RefName,
    ) -> Url {
        self.pattern
            .pattern_fill(|group| match group {
                "ref" => Some(
                    match refname {
                        RefName::Commit(commit) => commit,
                        RefName::Tag(tag) => tag,
                        RefName::Head => "HEAD",
                    }
                    .into(),
                ),
                "kind" => Some(
                    match refname {
                        RefName::Commit(..) | RefName::Head => &self.params.commit[0],
                        RefName::Tag(..) => &self.params.tag[0],
                    }
                    .into(),
                ),
                "tree" => Some(
                    match interest {
                        ContentInterest::Nav => &self.params.tree[0],
                        ContentInterest::Raw => &self.params.raw[0],
                    }
                    .into(),
                ),
                "path" => Some(href.encoded_path().into()),
                _ => None,
            })
            .include_after_path(href)
    }

    /// Try to extract a path (relative to repo root) from this link
    pub fn extract(&self, link: &Url) -> Option<(RelativeUrl, ContentInterest)> {
        let matches = self.pattern.pattern_test(Some("path"), link)?;

        if matches.matches.get("ref").map(|s| &**s) != Some("HEAD") {
            return None;
        }

        let href = matches.to_relative_url("path")?;

        let hint = if let Some(tree) = matches.matches.get("tree").map(|s| &**s) {
            if self.params.tree.iter().any(|plc| plc == tree) {
                ContentInterest::Nav
            } else if self.params.raw.iter().any(|plc| plc == tree) {
                ContentInterest::Raw
            } else {
                return None;
            }
        } else {
            return None;
        };

        debug!(?href, ?hint, "path matched");

        Some((href, hint))
    }
}

impl Show for Permalink {
    fn show(&self) -> impl std::fmt::Debug {
        self.pattern.show()
    }
}

enum RepoSource<'a> {
    Config(&'a gix_url::Url),
    Remote(gix_url::Url),
}

impl RepoSource<'_> {
    fn as_url(&self) -> &gix_url::Url {
        match self {
            Self::Config(u) => u,
            Self::Remote(u) => u,
        }
    }
}

impl VersionControl {
    #[instrument(level = "debug", skip_all)]
    pub fn try_from_git(config: &Config, root: &Path) -> Result<Result<Self>> {
        let repo = match Repository::open_ext(
            root,
            RepositoryOpenFlags::empty(),
            &[] as &[&std::ffi::OsStr],
        ) {
            Ok(repo) => repo,
            Err(err) => {
                let err = anyhow!("help: this preprocessor requires a git repository to work")
                    .context(format!("{err}"));
                return config.options.fail_on_warnings.adjusted(Ok(Err(err)));
            }
        };

        let root = repo.workdir().unwrap_or_else(|| repo.commondir());
        let root = root
            .canonicalize()
            .with_path_debug(root)
            .context("could not locate repo root")?
            .dir_to_url();

        trace!(repo = ?root.show());

        let refname =
            match get_git_head(&repo).context("could not get a tag or the commit hash to HEAD")? {
                Some(refname) => refname,
                None => {
                    let err = anyhow!("repo does not have any commit");
                    match config.options.fail_on_warnings.adjusted(Ok(Err(err))) {
                        Err(err) => return Err(err),
                        Ok(Err(err)) => {
                            warn!("{err}");
                            warn! { "links generated by the preprocessor will fallback to \
                            using `HEAD` as the reference" };
                            RefName::Head
                        }
                        Ok(Ok(())) => unreachable!(),
                    }
                }
            };

        let link = {
            let TemplateConfig { template, params } = &config.options.repo_url_template;

            let pattern = if let Some(template) = template {
                debug!("repo-url-template" = ?template.show());
                template.clone()
            } else {
                let remote = config.options.remote_name.as_deref().unwrap_or("origin");
                let repo = match find_git_remote(&repo, remote, config)
                    .context("error while trying to determine the URL format of permalinks")?
                {
                    Ok(repo) => repo,
                    Err(err) => {
                        return anyhow!(doc_link!(help = "how-to/remote-url"))
                            .context({
                                "help: set `output.html.git-repository-url` to a \
                                supported URL, or use `repo-url-template` option"
                            })
                            .context(err)
                            .context("could not determine the URL format of permalinks")
                            .pipe(Err)
                            .pipe(Ok)
                            .pipe(|result| config.options.fail_on_warnings.adjusted(result));
                    }
                };

                match derive_pattern(repo.as_url()) {
                    Ok(pattern) => pattern,
                    Err(err) => {
                        return anyhow!(doc_link!(help = "how-to/remote-url"))
                            .context({
                                "help: use the `repo-url-template` option \
                                to define a custom URL scheme"
                            })
                            .context(err)
                            .context({
                                let url = repo.as_url().to_string();
                                match repo {
                                    RepoSource::Config(..) => {
                                        format!("in `output.html.git-repository-url`: {url:?}")
                                    }
                                    RepoSource::Remote(..) => {
                                        format!("in git remote {remote:?}: {url:?}")
                                    }
                                }
                            })
                            .context("unsupported repository URL")
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
        info!("using format {:?}", link.pattern.show());
        info!("using ref {:?}", link.refname.show());

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
        Ok(Some(RefName::Tag(tag)))
    } else {
        let sha = head.id().to_string();
        Ok(Some(RefName::Commit(sha)))
    }
}

#[instrument(level = "debug", skip_all)]
fn find_git_remote<'a>(
    repo: &Repository,
    remote: &str,
    config: &'a Config,
) -> Result<Result<RepoSource<'a>>> {
    if let Some(ref url) = config.repo_url {
        debug!("git-repository-url" = ?url.to_string());
        Ok(Ok(RepoSource::Config(url)))
    } else {
        let repo = match repo.find_remote(remote).with_context(|| {
            format!("expected repo to have a remote named {remote:?}, but found none")
        }) {
            Ok(repo) => repo,
            Err(err) => return Ok(Err(err)),
        };
        let repo = match repo.url() {
            Ok(url) => url,
            Err(err) => {
                return Err(err)
                    .context(format!("expected remote {remote:?} to have a URL"))
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

#[instrument(level = "trace", skip_all)]
fn derive_pattern(url: &gix_url::Url) -> Result<Url> {
    let host = match url.host() {
        Some(host) => host,
        None => bail!("remote URL does not have a host"),
    };
    let path = url.path.to_string();

    fn is_on_domain(domain: &'static str, host: &str) -> bool {
        match host.strip_suffix(domain) {
            Some(sub) if sub.is_empty() || sub.ends_with('.') => {
                trace!("{host:?} is on domain {domain:?}");
                true
            }
            Some(..) | None => false,
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
        warn! { "help: it looks like you are using a self-hosted Tangled knot" };
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

impl Debug for Permalink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permalink")
            .field("pattern", &self.pattern.show())
            .field("refname", &self.refname)
            .finish_non_exhaustive()
    }
}

impl Debug for RepoPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("RepoPath");
        f.field("path", &self.relative.show_path())
            .field("is_dir", &self.is_dir)
            .field("is_ignored", &self.is_ignored);
        if self.url.query().is_some() || self.url.fragment().is_some() {
            f.field("url", &self.url.show()).finish()
        } else {
            f.finish_non_exhaustive()
        }
    }
}

impl Show for RefName {
    fn show(&self) -> impl Debug {
        std::fmt::from_fn(|f| match self {
            Self::Commit(hash) => write!(f, "{hash:.10} (from commit hash)"),
            Self::Tag(tag) => write!(f, "{tag} (from tag name)"),
            Self::Head => f.write_str("HEAD"),
        })
    }
}
