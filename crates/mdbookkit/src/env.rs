use std::{io::IsTerminal, ops::Deref, process::Command, sync::LazyLock};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use console::colors_enabled_stderr;
use serde::Deserialize;

use crate::{error::WithPathDebug, subprocess::CommandUtil};

#[macro_export]
macro_rules! env_var {
    ($name:ident $(, $extra:ident)*) => {
        pub(crate) static $name: ::std::sync::LazyLock<Option<String>> =
            ::std::sync::LazyLock::new(|| {
                std::env::var(stringify!($name))
                    $( .or_else(|_| std::env::var(stringify!($extra))) )*
                    .ok()
            });
    };
}

env_var!(MDBOOK_LOG, RUST_LOG);

env_var!(CI);
env_var!(FORCE_COLOR);
env_var!(NO_COLOR);

env_var!(MDBOOKKIT_TERM_PROGRESS);
env_var!(MDBOOKKIT_TERM_GRAPHICAL);

#[inline]
pub fn is_ci() -> Option<&'static str> {
    CI.truthy()
}

#[inline]
pub fn is_logging() -> bool {
    if MDBOOKKIT_TERM_PROGRESS.truthy().is_none() {
        MDBOOK_LOG.is_some() || is_ci().is_some() || !std::io::stderr().is_terminal()
    } else {
        false
    }
}

#[inline]
pub fn is_colored() -> bool {
    static IS_COLORED: LazyLock<bool> = LazyLock::new(|| {
        if FORCE_COLOR.truthy().is_some() {
            true
        } else if NO_COLOR.truthy().is_some() {
            false
        } else {
            colors_enabled_stderr()
        }
    });
    *IS_COLORED
}

pub trait TruthyStr {
    fn truthy(&self) -> Option<&str>;
}

impl<S: Deref<Target = str>> TruthyStr for Option<S> {
    fn truthy(&self) -> Option<&str> {
        let text = self.as_deref();
        if matches!(text, None | Some("")) {
            None
        } else {
            text
        }
    }
}

pub fn locate_project(command: Option<Command>) -> Result<Utf8PathBuf> {
    #[derive(Deserialize)]
    struct LocateProject {
        root: Utf8PathBuf,
    }

    let output = command
        .unwrap_or_else(|| Command::new("cargo").flag("locate-project", true))
        .args(["--message-format=json", "--workspace"])
        .run()
        .result()?
        .output()?;

    let LocateProject { root } = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("{:?}", String::from_utf8_lossy(&output.stdout)))
        .context("`cargo locate-project` returned unsupported data")?;

    let root = (root.parent())
        .with_path_debug(&root)
        .context("path to Cargo.toml should have a parent")?;

    Ok(root.to_owned())
}
