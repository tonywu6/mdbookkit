use std::path::Path;

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
            .tap_mut(|settings| settings.set_snapshot_path(path))
            .tap_mut(|settings| settings.set_prepend_module_to_snapshot(false))
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
