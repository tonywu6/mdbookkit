use std::path::Path;

use anyhow::Result;
use tap::Tap;

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
    pub name: String,
}

#[macro_export]
macro_rules! test_document {
    ($path:literal) => {
        $crate::TestDocument {
            source: include_str!($path),
            name: std::path::Path::new($path)
                .with_extension("")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        }
    };
}
