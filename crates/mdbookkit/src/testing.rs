use std::{ffi::OsString, path::Path, sync::LazyLock};

use anyhow::Result;
use serde::Deserialize;
use tap::{Pipe, Tap};
use tracing::info;
use url::Url;

use crate::url::{ExpectUrl, UrlFromPath, UrlToPath};

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
    pub fn cwd(&self) -> Url {
        CARGO_WORKSPACE_DIR
            .join(self.source_path)
            .expect_url()
            .join(".")
            .expect_url()
    }

    pub fn url(&self) -> Url {
        self.cwd().join(self.target_path).expect_url()
    }

    pub fn name(&self) -> String {
        let dir = Path::new(self.source_path)
            .with_extension("")
            .file_name()
            .expect("source_path should have a file name")
            .to_string_lossy()
            .into_owned();

        let url = self.url();
        let cwd = self.cwd().join(&format!("{dir}/")).expect_url();
        let rel = cwd.make_relative(&url).expect("both are file: URLs");

        if rel.starts_with("../") {
            let rel = CARGO_WORKSPACE_DIR
                .make_relative(&url)
                .expect("both are file: URLs");
            if rel.starts_with("../") {
                url.path_segments()
                    .expect("file: URL")
                    .next_back()
                    .expect("URL path not empty")
                    .to_owned()
            } else {
                rel
            }
        } else {
            rel
        }
    }
}

#[macro_export]
macro_rules! portable_snapshots {
    () => {
        $crate::testing::PortableSnapshots {
            source_path: std::path::Path::new(file!()),
        }
    };
}

#[derive(Debug)]
#[must_use]
pub struct PortableSnapshots {
    pub source_path: &'static Path,
}

impl PortableSnapshots {
    pub fn test<P, F, R>(&self, name: P, cb: F) -> Result<R>
    where
        P: AsRef<Path>,
        F: FnOnce(&str) -> R,
    {
        let Self { source_path } = self;

        let name = name.as_ref();

        let path = source_path.with_extension("").join("snaps");
        let path = CARGO_WORKSPACE_DIR
            .expect_path()
            .join(&*path.to_string_lossy());
        let path = if let Some(parent) = name.parent() {
            path.join(parent)
        } else {
            path
        };

        let name = name
            .file_name()
            .unwrap_or(name.as_os_str())
            .to_string_lossy();

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
            .bind(|| cb(&name));

        Ok(result)
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
            .map(|path| {
                Ok(Path::new(&path?)
                    .parent()
                    .expect("install path should not be root")
                    .to_owned())
            })
            .collect::<Result<Vec<_>>>()?,
    );

    path.push(
        CARGO_WORKSPACE_DIR
            .join("target")?
            .expect_path()
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
        .expect("cargo metadata must not fail")
        .pipe(|output| String::from_utf8(output.stdout))
        .expect("cargo metadata should output in utf8")
        .pipe(|output| serde_json::from_str::<CargoManifest>(&output))
        .expect("cargo metadata format should be correct")
        .pipe(|manifest| manifest.workspace_root.to_directory_url())
});

pub fn not_in_ci<D: std::fmt::Display>(because: D) -> bool {
    let ci = std::env::var("CI").unwrap_or("".into());
    if matches!(ci.as_str(), "" | "0" | "false") {
        info!("{because}");
        true
    } else {
        panic!("{because} but CI={ci}")
    }
}
