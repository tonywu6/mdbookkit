use std::{marker::PhantomData, sync::Arc};

pub struct Earth {
    pub r#type: &'static str,
    pub moon: PhantomData<()>,
    pub artemis: Arc<str>,
}

#[doc = include_str!("basic.md")]
pub mod basic {}
#[doc = include_str!("disambiguators.md")]
pub mod disambiguators {}
#[doc = include_str!("extern-crates.md")]
pub mod extern_crates {}
#[doc = include_str!("generics.md")]
pub mod generics {}
#[doc = include_str!("ignored.md")]
pub mod ignored {}
#[doc = include_str!("lints.md")]
pub mod lints {}
#[doc = include_str!("syntax.md")]
pub mod syntax {}
