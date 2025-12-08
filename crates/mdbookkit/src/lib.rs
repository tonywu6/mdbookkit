//! Toolkit for [`mdbook`].
//!
//! This is the lib documentation. If you are looking for the mdBook [preprocessors]
//! that this crate provides, visit <https://tonywu6.github.io/mdbookkit/> instead.
//!
//! At the moment, the sole purpose of this crate is to facilitate easier testing. Most of the APIs
//! are not designed for library use and are explicitly NOT stable.
//!
//! [preprocessors]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html

pub mod book;
pub mod diagnostics;
#[cfg(feature = "_testing")]
pub mod docs;
pub mod error;
pub mod logging;
pub mod markdown;
#[cfg(feature = "_testing")]
pub mod testing;
