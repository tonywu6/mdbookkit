use std::path::Path;

use anyhow::Result;
use tap::Tap;

pub struct PortableSnapshot {
    pub manifest: &'static str,
    pub module: &'static str,
}

impl PortableSnapshot {
    pub fn test<T: FnOnce()>(&self, cb: T) -> Result<()> {
        let Self { manifest, module } = self;

        let snapshot_dir = Path::new(manifest).join("tests").join("snapshots");

        let path = module
            .split("::")
            .fold(snapshot_dir, |dir, path| dir.join(path));

        insta::Settings::clone_current()
            .tap_mut(|settings| settings.set_snapshot_path(path))
            .tap_mut(|settings| settings.set_prepend_module_to_snapshot(false))
            .bind(cb);

        Ok(())
    }
}
