pub use anyhow::Context;

pub use mdbookkit::Diagnostics;

pub mod error {
    pub use mdbookkit::error::is_ci;
}
