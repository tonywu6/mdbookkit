//! Test helpers.

use std::{ffi::OsString, path::Path};

use anyhow::Result;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tap::{Pipe, Tap};
pub use url;
use url::Url;

pub static CARGO_WORKSPACE_DIR: Lazy<Url> = Lazy::new(|| {
    #[derive(Deserialize)]
    struct CargoManifest {
        workspace_root: String,
    }
    // https://github.com/mitsuhiko/insta/blob/b113499249584cb650150d2d01ed96ee66db6b30/src/runtime.rs#L67-L88
    std::process::Command::new(env!("CARGO"))
        .arg("metadata")
        .arg("--format-version=1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
        .pipe(|output| String::from_utf8(output.stdout))
        .unwrap()
        .pipe(|outout| serde_json::from_str::<CargoManifest>(&outout))
        .unwrap()
        .pipe(|manifest| Url::from_directory_path(manifest.workspace_root))
        .unwrap()
});

#[macro_export]
macro_rules! portable_snapshots {
    () => {
        $crate::PortableSnapshots {
            manifest: env!("CARGO_MANIFEST_DIR"),
            module: module_path!(),
        }
    };
}

#[derive(Debug)]
#[must_use]
pub struct PortableSnapshots {
    pub manifest: &'static str,
    pub module: &'static str,
}

impl PortableSnapshots {
    pub fn test<T: FnOnce() -> R, R>(&self, cb: T) -> Result<R> {
        let Self { manifest, module } = self;

        let snapshot_dir = Path::new(manifest).join("tests").join("snaps");

        let path = module
            .split("::")
            .fold(snapshot_dir, |dir, path| dir.join(path));

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

pub struct TestDocument {
    pub source: &'static str,
    pub file: Url,
    pub name: String,
}

#[macro_export]
macro_rules! test_document {
    ($path:literal) => {
        $crate::TestDocument {
            source: include_str!($path),
            file: $crate::CARGO_WORKSPACE_DIR
                .join(file!())
                .unwrap()
                .join($path)
                .unwrap(),
            name: std::path::Path::new($path)
                .with_extension("")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    };
}

pub fn may_skip<D: std::fmt::Display>(because: D) -> bool {
    let ci = std::env::var("CI").unwrap_or("".into());
    if matches!(ci.as_str(), "" | "0" | "false") {
        log::info!("{because}");
        true
    } else {
        panic!("{because} but CI={ci}")
    }
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
