use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cargo_toml::{Manifest, Product};
use lsp_types::Url;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use shlex::Shlex;
use tokio::process::Command;
use tracing::debug;

use mdbookkit::{
    error::{IntoAnyhow, OnWarning},
    markdown::default_markdown_options,
    url::{ExpectUrl, UrlFromPath},
};

use crate::markdown;

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
    /// Setting this will enable caching. The preprocessor will skip running
    /// rust-analyzer if cache hits.
    #[serde(default)]
    #[arg(long, value_name("PATH"), value_hint(clap::ValueHint::DirPath))]
    pub cache_dir: Option<PathBuf>,

    /// Exit with a non-zero status code when some links fail to resolve.
    #[serde(default)]
    #[arg(long, value_enum, value_name("MODE"), default_value_t = Default::default())]
    pub fail_on_warnings: OnWarning,

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

#[derive(Clone)]
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
            .context("Failed to get the current working directory")?
            .canonicalize()
            .context("Failed to resolve `manifest-dir` to a path")?;

        let (crate_dir, entrypoint) = {
            let manifest_path = LocateProject::package(&cwd)
                .context("Preprocessor requires a Cargo project to run rust-analyzer")
                .context("Failed to determine the current Cargo project")?
                .root;

            let manifest = Manifest::from_path(&manifest_path)
                .and_then(|mut m| m.complete_from_path(&manifest_path).and(Ok(m)))
                .context("Failed to read from Cargo.toml")?;

            let crate_dir = manifest_path
                .parent()
                .expect("manifest_path should have a parent")
                .to_directory_url();

            if let Some(Product {
                path: Some(ref lib),
                ..
            }) = manifest.lib
            {
                let entry = crate_dir.join(lib).expect_url();
                Ok((crate_dir, entry))
            } else if let Some(bin) = manifest.bin.iter().find_map(|bin| bin.path.as_ref()) {
                let entry = crate_dir.join(bin).expect_url();
                Ok((crate_dir, entry))
            } else {
                let err = Err(anyhow!(
                    "help: resolved Cargo.toml is {}",
                    manifest_path.display()
                ));
                if manifest.workspace.is_some() {
                    err.context(
                        "help: for usage in a workspace, set option \
                        `manifest-dir` to the root of a member crate",
                    )
                } else {
                    err
                }
                .context("Cargo.toml does not have any lib or bin target")
            }
        }?;

        let source_dir = LocateProject::workspace(cwd)
            .context("Failed to locate the current Cargo project")?
            .directory()
            .to_directory_url();

        let temp_dir = match config.cache_dir.clone() {
            Some(path) => Some(TempDir::Persistent(path)),
            None => tempfile::TempDir::new()
                .ok()
                .map(Arc::new)
                .map(TempDir::Transient),
        }
        .context("Failed to obtain a temporary directory")?;

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
        markdown::stream(source, default_markdown_options())
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
        debug!("reading temp file from {}", path.display());
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save_temp<T, P>(&self, path: P, temp: &T) -> Result<()>
    where
        T: Serialize,
        P: AsRef<Path>,
    {
        let path = self.temp_dir.as_ref().join(path);
        debug!("saving temp file to {}", path.display());
        let text = serde_json::to_string(&temp).context("Failed to serialize cache data")?;
        std::fs::create_dir_all(path.parent().expect("temp dir should have a parent"))
            .context("Failed to create cache dir")?;
        std::fs::write(path, text).context("Failed to write to cache dir")?;
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
    fn directory(&self) -> &Path {
        self.root
            .parent()
            .expect("path to Cargo.toml should have a parent")
    }

    fn package<P: AsRef<Path>>(cwd: P) -> Result<Self> {
        std::process::Command::new("cargo")
            .arg("locate-project")
            .arg("--message-format=json")
            .current_dir(cwd)
            .output()
            .context("Failed to run `cargo locate-project`, is cargo installed?")
            .and_then(Self::parse)
    }

    fn workspace<P: AsRef<Path>>(cwd: P) -> Result<Self> {
        std::process::Command::new("cargo")
            .arg("locate-project")
            .arg("--message-format=json")
            .arg("--workspace")
            .current_dir(cwd)
            .output()
            .context("Failed to run `cargo locate-project`, is cargo installed?")
            .and_then(Self::parse)
    }

    fn parse(output: std::process::Output) -> Result<Self> {
        let std::process::Output {
            status,
            stderr,
            stdout,
        } = output;
        if status.success() {
            (String::from_utf8(stdout).anyhow())
                .and_then(|output| serde_json::from_str(&output).anyhow())
                .context("Could not parse `cargo locate-project` output")
        } else {
            Err(anyhow!(String::from_utf8_lossy(&stderr).into_owned()))
                .context("`cargo locate-project` did not run successfully")
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
                    .context("Unexpected empty string for option `rust-analyzer`")?;
                let mut cmd = Command::new(executable);
                cmd.args(words);
                Ok(cmd)
            }
            Self::VsCode(cmd) => {
                debug!("using rust-analyzer from {}", cmd.display());
                Ok(Command::new(cmd))
            }
            Self::Path => {
                debug!("using rust-analyzer on PATH");
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

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            temp_dir,
            crate_dir,
            source_dir,
            entrypoint,
            config,
        } = self;
        f.debug_struct("Environment")
            .field("crate_dir", &format_args!("\"{crate_dir}\""))
            .field("source_dir", &format_args!("\"{source_dir}\""))
            .field("entrypoint", &format_args!("\"{entrypoint}\""))
            .field("config", &config)
            .field("temp_dir", &temp_dir)
            .finish()
    }
}
