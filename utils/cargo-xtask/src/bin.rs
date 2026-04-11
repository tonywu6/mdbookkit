use std::env::{current_dir, set_current_dir};

use anyhow::{Context, Result};
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
        let Self { cwd, bin, args } = self;

        if let Some(cwd) = cwd {
            let dir = current_dir()?;
            let dir = dir.join(cwd).canonicalize()?;
            set_current_dir(dir)?;
        }

        let packages = metadata::get_binary_packages()?;

        if bin == "--install" {
            for package in packages {
                binary::install(package)?;
            }
            return Ok(());
        }

        let bin = if bin.starts_with("cargo-") {
            &[bin] as &[_]
        } else {
            &[format!("cargo-{bin}"), bin]
        };

        let pkg = packages
            .into_iter()
            .find(|pkg| {
                bin.iter().any(|bin| {
                    &pkg.package == bin
                        || (pkg.bin_target.as_ref())
                            .map(|name| name == bin)
                            .unwrap_or(false)
                })
            })
            .with_context(|| format!("no package provides the binary {bin:?}"))?;

        let bin = binary::install(pkg)?;

        binary::run(bin, args)?;

        Ok(())
    }
}
