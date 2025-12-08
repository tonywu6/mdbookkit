use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use cargo_toml::{Manifest, Product};
use lsp_types::Url;
use pulldown_cmark::Options;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use shlex::Shlex;
use tap::Pipe;
use tokio::process::Command;

use mdbookkit::{error::OnWarning, markdown::mdbook_markdown_options};

use super::markdown;

/// Configuration for the preprocessor.
///
/// This is both deserialized from book.toml and parsed from the command line.
///
/// Doc comments for attributes populate the `configuration.md` page in the docs.
#[derive(clap::Parser, Deserialize, Debug, Default, Clone)]
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
    #[arg(
        long,
        value_name("COMMAND"),
        value_hint(clap::ValueHint::CommandString)
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
    #[serde(default)]
    #[arg(long, value_delimiter(','), value_name("FEATURES"))]
    pub cargo_features: Vec<String>,

    /// Directory from which to search for a Cargo project.
    ///
    /// By default, the current working directory is used. Use this option to specify a
    /// different directory.
    ///
    /// The processor requires the Cargo.toml of a package to work. If you are working
    /// on a Cargo workspace, set this to the relative path to a member crate.
    #[serde(default)]
    #[arg(long, value_name("PATH"), value_hint(clap::ValueHint::DirPath))]
    pub manifest_dir: Option<PathBuf>,

    /// Directory in which to persist build cache.
    ///
    /// Setting this will enable caching. Will skip rust-analyzer if cache hits.
    #[serde(default)]
    #[arg(long, value_name("PATH"), value_hint(clap::ValueHint::DirPath))]
    pub cache_dir: Option<PathBuf>,

    /// Exit with a non-zero status code when some links fail to resolve.
    ///
    /// Warnings are always printed to the console regardless of this option.
    #[serde(default)]
    #[arg(long, value_enum, value_name("MODE"), default_value_t = Default::default())]
    pub fail_on_warnings: OnWarning,

    /// Whether to enable punctuations like smart quotes `“”`.
    ///
    /// This is only meaningful if your links happen to have visible text that has
    /// specific punctuation. The processor otherwise passes through the rest of your
    /// Markdown source untouched.
    ///
    /// **In `book.toml`** — this option is not needed because
    /// `output.html.smart-punctuation` is honored.
    #[serde(default)]
    #[arg(long)]
    pub smart_punctuation: bool,

    #[serde(default)]
    #[arg(long, hide = true)]
    pub prefer_local_links: bool,

    /// Timeout in seconds to wait for rust-analyzer to finish indexing.
    #[serde(default)]
    #[arg(long, value_name("SECONDS"), default_value("60"))]
    pub rust_analyzer_timeout: Option<u64>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[arg(skip)]
    pub after: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[arg(skip)]
    pub before: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[arg(skip)]
    pub renderers: Option<Vec<String>>,

    #[allow(unused)]
    #[serde(default)]
    #[doc(hidden)]
    #[arg(skip)]
    pub command: Option<String>,
}

impl Config {
    pub fn rust_analyzer_timeout(&self) -> Duration {
        Duration::from_secs(self.rust_analyzer_timeout.unwrap_or(60))
    }
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
            .unwrap_or_else(std::env::current_dir)
            .context("failed to get the current working directory")?
            .canonicalize()
            .context("failed to resolve `manifest-dir` to a path")?;

        let (crate_dir, entrypoint) = {
            let manifest_path = LocateProject::package(&cwd)
                .context("preprocessor requires a Cargo project to run rust-analyzer")
                .context("failed to determine the current Cargo project")?
                .root;

            let manifest = Manifest::from_path(&manifest_path)
                .and_then(|mut m| m.complete_from_path(&manifest_path).and(Ok(m)))
                .context("failed to read Cargo.toml")?;

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
                let err = Err(anyhow!(
                    "help: resolved Cargo.toml is {}",
                    manifest_path.display()
                ));
                if manifest.workspace.is_some() {
                    err.context("help: to use in a workspace, set `manifest-dir` option to root of a member crate")
                } else {
                    err
                }
                .context("Cargo.toml does not have any lib or bin target")
            }
        }?;

        let source_dir = LocateProject::workspace(cwd)
            .context("failed to locate the current Cargo project")?
            .root
            .parent()
            .unwrap()
            .pipe(Url::from_directory_path)
            .unwrap();

        let temp_dir = match config.cache_dir.clone() {
            Some(path) => Some(TempDir::Persistent(path)),
            None => tempfile::TempDir::new()
                .ok()
                .map(Arc::new)
                .map(TempDir::Transient),
        }
        .context("failed to obtain a temporary directory")?;

        Ok(Self {
            temp_dir,
            crate_dir,
            source_dir,
            entrypoint,
            config,
        })
    }

    pub fn which(&self) -> RustAnalyzer<'_> {
        if let Some(command) = self.config.rust_analyzer.as_deref() {
            RustAnalyzer::Custom(command)
        } else if let Some(command) = find_code_extension() {
            RustAnalyzer::VsCode(command)
        } else {
            RustAnalyzer::Path
        }
    }

    pub fn markdown<'a>(&self, source: &'a str) -> markdown::MarkdownStream<'a> {
        let options = if self.config.smart_punctuation {
            Options::ENABLE_SMART_PUNCTUATION
        } else {
            Options::empty()
        };
        markdown::stream(source, options.union(mdbook_markdown_options()))
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
        std::process::Command::new("cargo")
            .arg("locate-project")
            .arg("--message-format=json")
            .current_dir(cwd)
            .output()
            .context("failed to run `cargo locate-project`, is cargo installed?")
            .and_then(Self::parse)
    }

    fn workspace<P: AsRef<Path>>(cwd: P) -> Result<Self> {
        std::process::Command::new("cargo")
            .arg("locate-project")
            .arg("--message-format=json")
            .arg("--workspace")
            .current_dir(cwd)
            .output()
            .context("failed to run `cargo locate-project`, is cargo installed?")
            .and_then(Self::parse)
    }

    fn parse(output: std::process::Output) -> Result<Self> {
        if output.status.success() {
            String::from_utf8(output.stdout)?
                .pipe(|outout| serde_json::from_str::<Self>(&outout))?
                .pipe(Ok)
        } else {
            bail!(String::from_utf8_lossy(&output.stderr).into_owned());
        }
    }
}

pub enum RustAnalyzer<'a> {
    Custom(&'a str),
    VsCode(PathBuf),
    Path,
}

impl<'a> RustAnalyzer<'a> {
    pub fn command(self) -> Result<Command> {
        match self {
            Self::Custom(cmd) => {
                let mut words = Shlex::new(cmd);
                let executable = words
                    .next()
                    .context("unexpected empty string for option `rust-analyzer`")?;
                let mut cmd = Command::new(executable);
                cmd.args(words);
                Ok(cmd)
            }
            Self::VsCode(cmd) => {
                log::debug!("using rust-analyzer from {}", cmd.display());
                Ok(Command::new(cmd))
            }
            Self::Path => {
                log::debug!("using rust-analyzer on PATH");
                Ok(Command::new("rust-analyzer"))
            }
        }
    }
}

/// Look for rust-analyzer binary from the VS Code extension based on locations
/// described in <https://rust-analyzer.github.io/book/vs_code.html>.
pub fn find_code_extension() -> Option<PathBuf> {
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
