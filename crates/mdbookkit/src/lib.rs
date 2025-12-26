#![warn(clippy::unwrap_used)]
#![doc = include_str!("../README.md")]

pub mod book;
pub mod diagnostics;
#[cfg(feature = "_testing")]
pub mod docs;
pub mod env;
pub mod error;
pub mod logging;
pub mod markdown;
#[cfg(feature = "_testing")]
pub mod testing;
pub mod url;

// referenced in docs
#[doc(hidden)]
pub use diagnostics::Diagnostics;
