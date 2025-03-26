//! This crate only for providing a context for mdbook-rustdoc-link to resolve items from

#![allow(unused)]

use anyhow::Context;

use mdbook_rustdoc_link::Resolver;

mod env {
    pub use mdbook_rustdoc_link::env::Config;
}
