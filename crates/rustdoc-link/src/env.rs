use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use cargo_toml::{Manifest, Product};
use lsp_types::Url;
use serde::{Deserialize, Serialize};
use shlex::Shlex;
use tap::Pipe;
use tokio::process::Command;

use crate::BuildOptions;

#[derive(Debug, Clone)]
pub struct Environment {
    pub cache_dir: TempDir,
    pub crate_dir: Url,
    pub source_dir: Url,
    pub entrypoint: Url,
    pub build_opts: BuildOptions,
}

impl Environment {
    pub fn new(build_opts: BuildOptions) -> Result<Self> {
        let cwd = build_opts
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

        let cache_dir = build_opts
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
            crate_dir,
            cache_dir,
            source_dir,
            entrypoint,
            build_opts,
        })
    }

    pub fn command(&self) -> Result<Command> {
        let Some(command) = self.build_opts.rust_analyzer.as_deref() else {
            return Ok(Command::new("rust-analyzer"));
        };

        let mut words = Shlex::new(command);

        let executable = words
            .next()
            .context("unexpected empty string for option `rust-analyzer`")?;

        let mut cmd = Command::new(executable);

        cmd.args(words);

        Ok(cmd)
    }

    pub fn read_cache<C: for<'de> Deserialize<'de>>(&self) -> Result<C> {
        let path = self.cache_dir.as_ref().join("cache.json");
        let text = std::fs::read_to_string(&path).context("failed to read cache file")?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save_cache<C: Serialize>(&self, cache: C) -> Result<()> {
        let path = self.cache_dir.as_ref().join("cache.json");
        let text = serde_json::to_string(&cache).context("failed to serialize cache")?;
        std::fs::create_dir_all(path.parent().unwrap()).context("failed to create cache dir")?;
        std::fs::write(path, text).context("failed to write cache")?;
        Ok(())
    }
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
                .context("failed to locate-project")
                .pipe(Err)
        }
    }
}
