pub use anyhow::Context;

pub use mdbookkit::Diagnostics;

pub mod env {
    pub use mdbookkit::env::is_ci;
}
