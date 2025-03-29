//! This crate is only here to provide a context for mdbook-rustdoc-link to resolve items from.

#![allow(unused)]

use anyhow::Context;

use mdbookkit::bin::rustdoc_link::Resolver;

mod env {
    pub use mdbookkit::bin::rustdoc_link::env::Config;
}
