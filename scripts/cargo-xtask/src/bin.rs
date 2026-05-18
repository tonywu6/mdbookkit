use std::{
    env::{current_dir, join_paths, set_current_dir, set_var, split_paths, var_os},
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use cargo_run_bin::{binary, metadata};

#[derive(clap::Parser, Debug)]
pub struct Command {
    #[arg(long)]
    cwd: Option<String>,
    bin: String,
    #[clap(trailing_var_arg(true), allow_hyphen_values(true))]
    args: Vec<String>,
}

impl Command {
    pub fn run(self) -> Result<()> {
        let Self {
            cwd,
            bin: name,
            args,
        } = self;

        if let Some(cwd) = cwd {
            let dir = current_dir()?;
            let dir = dir.join(cwd).canonicalize()?;
            set_current_dir(dir)?;
        }

        let packages = metadata::get_binary_packages()?;

        let mut spawn = None;
        let mut paths = Vec::with_capacity(packages.len() + 1);

        let candidates = if name.starts_with("cargo-") {
            std::slice::from_ref(&name)
        } else {
            &[format!("cargo-{name}"), name.clone()]
        };

        for package in packages {
            let matched = candidates.iter().any(|bin| {
                &package.package == bin
                    || (package.bin_target.as_ref())
                        .map(|name| name == bin)
                        .unwrap_or(false)
            });
            let path = binary::install(package)?;
            if let Some(dir) = Path::new(&path).parent() {
                paths.push(dir.as_os_str().to_owned());
            }
            if matched {
                spawn = Some(path);
            }
        }

        if name == "--install" {
            return Ok(());
        }

        let Some(path) = spawn else {
            bail!("no package provides the binary {name:?}");
        };

        let path_env = var_os("PATH").unwrap_or_default();
        let path_env = split_paths(&path_env);
        #[cfg(windows)] // cargo-xtask
        let path_env = path_env.filter(|p| !p.ends_with("target/release"));
        let path_env = path_env.map(PathBuf::into_os_string);

        let paths = paths.into_iter().chain(path_env);
        let paths = join_paths(paths)?;
        unsafe {
            set_var("PATH", paths);
        };

        binary::run(path, args)?;

        Ok(())
    }
}
