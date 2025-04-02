//! Toolkit for [`mdbook`].
//!
//! This is the lib documentation. If you are looking for the mdBook [preprocessors]
//! that this crate provides, visit <https://tonywu6.github.io/mdbookkit/> instead.
//!
//! [preprocessors]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html

#[cfg(feature = "common-logger")]
pub mod diagnostics;
pub mod env;
pub mod logging;
pub mod markdown;

pub mod bin;
