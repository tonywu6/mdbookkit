use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use cargo_toml::{Manifest, Product};
use lsp_types::Url;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use shlex::Shlex;
use tap::Pipe;
use tokio::process::Command;

use crate::markdown::{markdown_parser, MarkdownStream};

#[derive(clap::Parser, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[arg(long)]
    #[serde(default)]
    pub rust_analyzer: Option<String>,

    #[arg(long)]
    #[serde(default)]
    pub manifest_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,

    #[arg(long)]
    #[serde(default)]
    pub smart_punctuation: bool,

    #[arg(long)]
    #[serde(default)]
    pub prefer_local_links: bool,
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

        std::fs::write(temp_dir.as_ref().join(".gitignore"), "/*").ok();

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

    pub fn markdown<'a>(&self, source: &'a str) -> MarkdownStream<'a> {
        let Config {
            smart_punctuation, ..
        } = self.config;
        markdown_parser(source, smart_punctuation)
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
