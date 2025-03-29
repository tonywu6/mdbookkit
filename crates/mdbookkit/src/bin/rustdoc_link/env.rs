use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use cargo_toml::{Manifest, Product};
#[cfg(feature = "common-cli")]
use clap::ValueHint;
use lsp_types::Url;
use pulldown_cmark::Options;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use shlex::Shlex;
use tap::Pipe;
use tokio::process::Command;

use crate::{env::ErrorHandling, markdown::mdbook_markdown};

use super::markdown;

#[derive(Deserialize, Debug, Default, Clone)]
#[cfg_attr(feature = "common-cli", derive(clap::Parser))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// Command to use for spawning rust-analyzer.
    ///
    /// By default, prebuilt binary from the VS Code extension is tried. If that doesn't
    /// exist, it is assumed that rust-analyzer is on `PATH`. Use this option to override
    /// this behavior completely.
    ///
    /// The command string will be tokenized by [shlex], so you can include arguments in it.
    #[serde(default)]
    #[cfg_attr(
        feature = "common-cli",
        arg(long, value_name("COMMAND"), value_hint(ValueHint::CommandString))
    )]
    pub rust_analyzer: Option<String>,

    /// List of features to activate when running rust-analyzer.
    ///
    /// This is just the `rust-analyzer.cargo.features` config.
    ///
    /// **In `book.toml`** — to enable all features, use `["all"]`.
    ///
    /// **For CLI** — to enable multiple features, specify as
    /// comma-separated values, or specify multiple times; to enable all features,
    /// specify `--cargo-features all`.
    #[cfg_attr(feature = "common-cli", arg(long, value_name("FEATURES")))]
    #[serde(default)]
    pub cargo_features: Vec<String>,

    /// Directory from which to search for a Cargo project.
    ///
    /// By default, the current working directory is used. Use this option to specify a
    /// different directory.
    ///
    /// The processor requires the Cargo.toml of a package to work. If you are working
    /// on a Cargo workspace, set this to the relative path to a member crate.
    #[cfg_attr(
        feature = "common-cli",
        arg(long, value_name("PATH"), value_hint(ValueHint::DirPath))
    )]
    #[serde(default)]
    pub manifest_dir: Option<PathBuf>,

    /// Directory in which to persist build cache.
    ///
    /// Setting this will enable caching. Will skip rust-analyzer if cache hits.
    #[cfg_attr(
        feature = "common-cli",
        arg(long, value_name("PATH"), value_hint(ValueHint::DirPath))
    )]
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    /// Whether to exit with failure when some links fail to resolve.
    ///
    /// Warnings are always emitted for unresolved links regardless of this option.
    #[serde(default)]
    #[cfg_attr(feature = "common-cli", arg(long, value_enum, value_name("MODE"), default_value_t = Default::default()))]
    pub fail_on_unresolved: ErrorHandling,

    /// Whether to enable punctuations like smart quotes `“”`.
    ///
    /// This is only meaningful if your links happen to have visible text that has
    /// specific punctuation. The processor otherwise passes through the rest of your
    /// Markdown source.
    ///
    /// **In `book.toml`** — this option is not needed because
    /// `output.html.smart-punctuation` is honored.
    #[serde(default)]
    #[cfg_attr(feature = "common-cli", arg(long))]
    pub smart_punctuation: bool,

    #[serde(default)]
    #[cfg_attr(feature = "common-cli", arg(long, hide = true))]
    pub prefer_local_links: bool,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[cfg_attr(feature = "common-cli", arg(skip))]
    pub after: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[cfg_attr(feature = "common-cli", arg(skip))]
    pub before: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[cfg_attr(feature = "common-cli", arg(skip))]
    pub renderers: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[cfg_attr(feature = "common-cli", arg(skip))]
    pub command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub temp_dir: TempDir,
    pub crate_dir: Url,
    pub source_dir: Url,
    pub entrypoint: Url,
    pub config: Config,
}

impl Environment {
    pub fn new(config: Config) -> Result<Self> {
        let cwd = config
            .manifest_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)?
            .canonicalize()?;

        let (crate_dir, entrypoint) = {
            let manifest_path = LocateProject::package(&cwd)?.root;

            let manifest = {
                let mut manifest = Manifest::from_path(&manifest_path)?;
                manifest.complete_from_path(&manifest_path)?;
                manifest
            };

            let crate_dir = manifest_path
                .parent()
                .unwrap()
                .pipe(Url::from_directory_path)
                .unwrap();

            if let Some(Product {
                path: Some(ref lib),
                ..
            }) = manifest.lib
            {
                let entry = crate_dir.join(lib)?;
                Ok((crate_dir, entry))
            } else if let Some(bin) = manifest.bin.iter().find_map(|bin| bin.path.as_ref()) {
                let entry = crate_dir.join(bin)?;
                Ok((crate_dir, entry))
            } else {
                Err(anyhow!(
                    "help: resolved Cargo.toml is {}",
                    manifest_path.display()
                ))
                .pipe(|r| {
                    if manifest.workspace.is_some() {
                        r.context("help: to use in a workspace, set `manifest-dir` option to root of a member crate")
                    } else {
                        r
                    }
                })
                .context("Cargo.toml does not have any lib or bin target")
            }
        }?;

        let source_dir = LocateProject::workspace(cwd)?
            .root
            .parent()
            .unwrap()
            .pipe(Url::from_directory_path)
            .unwrap();

        let temp_dir = config
            .cache_dir
            .clone()
            .map(TempDir::Persistent)
            .or_else(|| {
                tempfile::TempDir::new()
                    .ok()
                    .map(Arc::new)
                    .map(TempDir::Transient)
            })
            .context("failed to obtain a temporary directory")?;

        Ok(Self {
            temp_dir,
            crate_dir,
            source_dir,
            entrypoint,
            config,
        })
    }

    pub fn command(&self) -> Result<Command> {
        if let Some(command) = self.config.rust_analyzer.as_deref() {
            let mut words = Shlex::new(command);
            let executable = words
                .next()
                .context("unexpected empty string for option `rust-analyzer`")?;
            let mut cmd = Command::new(executable);
            cmd.args(words);
            Ok(cmd)
        } else if let Some(extension) = find_code_extension() {
            log::debug!("using rust-analyzer from {}", extension.display());
            Ok(Command::new(extension))
        } else {
            Ok(Command::new("rust-analyzer"))
        }
    }

    pub fn markdown<'a>(&self, source: &'a str) -> markdown::MarkdownStream<'a> {
        let options = if self.config.smart_punctuation {
            Options::ENABLE_SMART_PUNCTUATION
        } else {
            Options::empty()
        };
        markdown::stream(source, options.union(mdbook_markdown()))
    }

    pub fn emit_config(&self) -> EmitConfig {
        let Config {
            prefer_local_links, ..
        } = self.config;
        EmitConfig { prefer_local_links }
    }

    pub fn load_temp<T, P>(&self, path: P) -> Result<T>
    where
        T: DeserializeOwned,
        P: AsRef<Path>,
    {
        let path = self.temp_dir.as_ref().join(path);
        let text = std::fs::read_to_string(&path).context("failed to read from cache dir")?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save_temp<T, P>(&self, path: P, temp: &T) -> Result<()>
    where
        T: Serialize,
        P: AsRef<Path>,
    {
        let path = self.temp_dir.as_ref().join(path);
        let text = serde_json::to_string(&temp).context("failed to serialize cache data")?;
        std::fs::create_dir_all(path.parent().unwrap()).context("failed to create cache dir")?;
        std::fs::write(path, text).context("failed to write to cache dir")?;
        Ok(())
    }
}

pub struct EmitConfig {
    pub prefer_local_links: bool,
}

#[derive(Debug, Clone)]
pub enum TempDir {
    Persistent(PathBuf),
    Transient(Arc<tempfile::TempDir>),
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        match self {
            Self::Persistent(p) => p.as_ref(),
            Self::Transient(p) => p.deref().as_ref(),
        }
    }
}

#[derive(Deserialize)]
struct LocateProject {
    root: PathBuf,
}

impl LocateProject {
    fn package<P: AsRef<Path>>(cwd: P) -> Result<Self> {
        std::process::Command::new(env!("CARGO"))
            .arg("locate-project")
            .arg("--message-format=json")
            .current_dir(cwd)
            .output()?
            .pipe(Self::parse)
    }

    fn workspace<P: AsRef<Path>>(cwd: P) -> Result<Self> {
        std::process::Command::new(env!("CARGO"))
            .arg("locate-project")
            .arg("--message-format=json")
            .arg("--workspace")
            .current_dir(cwd)
            .output()?
            .pipe(Self::parse)
    }

    fn parse(output: std::process::Output) -> Result<Self> {
        if output.status.success() {
            String::from_utf8(output.stdout)?
                .pipe(|outout| serde_json::from_str::<Self>(&outout))?
                .pipe(Ok)
        } else {
            anyhow!(String::from_utf8_lossy(&output.stderr).into_owned())
                .context("help: a Cargo project is needed to run rust-analyzer in")
                .context("failed to locate a Cargo project")
                .pipe(Err)
        }
    }
}

fn find_code_extension() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    [
        home.join(".vscode/extensions"),
        home.join(".vscode-server/extensions"),
    ]
    .iter()
    .flat_map(|p| p.read_dir().ok())
    .flatten()
    .flatten()
    .find_map(|extension| {
        if extension
            .file_name()
            .to_string_lossy()
            .starts_with("rust-lang.rust-analyzer-")
        {
            Some(extension.path().join("server/rust-analyzer"))
        } else {
            None
        }
    })
}
