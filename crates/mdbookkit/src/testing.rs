use std::{ffi::OsString, path::Path, sync::LazyLock};

use anyhow::Result;
use log::LevelFilter;
use serde::Deserialize;
use tap::{Pipe, Tap};
use url::Url;

use crate::logging::ConsoleLogger;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TestDocument {
    pub source_path: &'static str,
    pub target_path: &'static str,
    pub content: &'static str,
}

#[macro_export]
macro_rules! test_document {
    ($path:literal) => {
        $crate::testing::TestDocument {
            source_path: file!(),
            target_path: $path,
            content: include_str!($path),
        }
    };
}

impl TestDocument {
    pub fn url(&self) -> Url {
        CARGO_WORKSPACE_DIR
            .join(self.source_path)
            .unwrap()
            .join(self.target_path)
            .unwrap()
    }

    pub fn name(&self) -> String {
        std::path::Path::new(self.target_path)
            .with_extension("")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
}

#[macro_export]
macro_rules! portable_snapshots {
    () => {
        $crate::testing::PortableSnapshots {
            file: std::path::Path::new(file!()),
        }
    };
}

#[derive(Debug)]
#[must_use]
pub struct PortableSnapshots {
    pub file: &'static Path,
}

impl PortableSnapshots {
    pub fn test<T: FnOnce() -> R, R>(&self, cb: T) -> Result<R> {
        let Self { file } = self;

        let path = file.with_extension("").join("snaps");
        let path = CARGO_WORKSPACE_DIR
            .join(&path.to_string_lossy())
            .unwrap()
            .to_file_path()
            .unwrap();

        let result = insta::Settings::clone_current()
            .tap_mut(|s| s.set_snapshot_path(path))
            .tap_mut(|s| s.set_prepend_module_to_snapshot(false))
            .tap_mut(|s| {
                s.set_filters(vec![
                    (r"file:///[A-Z]:/", "file:///"), // windows paths
                    (r"(?m)^(\s+)\d{1} ", "$1  "),    // miette line numbers
                    (r"(?m)^(\s+)\d{2} ", "$1   "),
                    (r"(?m)^(\s+)\d{3} ", "$1    "),
                ])
            })
            .bind(cb);

        Ok(result)
    }
}

pub fn setup_logging(name: &str) {
    ConsoleLogger::try_install(name).ok();
    log::set_max_level(LevelFilter::Debug);
}

pub fn setup_paths() -> Result<OsString> {
    let mut path = if let Some(path) = std::env::var_os("PATH") {
        std::env::split_paths(&path)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    } else {
        vec![]
    };

    path.push(Path::new(env!("CARGO_HOME")).join("bin"));

    path.extend(
        cargo_run_bin::metadata::get_binary_packages()?
            .into_iter()
            .map(cargo_run_bin::binary::install)
            .map(|path| Ok(Path::new(&path?).parent().unwrap().to_owned()))
            .collect::<Result<Vec<_>>>()?,
    );

    path.push(
        CARGO_WORKSPACE_DIR
            .join("target")?
            .to_file_path()
            .unwrap()
            .join(if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }),
    );

    Ok(std::env::join_paths(path.into_iter().rev())?)
}

pub static CARGO_WORKSPACE_DIR: LazyLock<Url> = LazyLock::new(|| {
    #[derive(Deserialize)]
    struct CargoManifest {
        workspace_root: String,
    }
    // https://github.com/mitsuhiko/insta/blob/b113499249584cb650150d2d01ed96ee66db6b30/src/runtime.rs#L67-L88
    std::process::Command::new(env!("CARGO"))
        .arg("metadata")
        .args(["--format-version=1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
        .pipe(|output| String::from_utf8(output.stdout))
        .unwrap()
        .pipe(|output| serde_json::from_str::<CargoManifest>(&output))
        .unwrap()
        .pipe(|manifest| Url::from_directory_path(manifest.workspace_root))
        .unwrap()
});

pub fn not_in_ci<D: std::fmt::Display>(because: D) -> bool {
    let ci = std::env::var("CI").unwrap_or("".into());
    if matches!(ci.as_str(), "" | "0" | "false") {
        log::info!("{because}");
        true
    } else {
        panic!("{because} but CI={ci}")
    }
}
