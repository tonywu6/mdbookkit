pub struct Earth {
    pub r#type: &'static str,
    pub moon: std::marker::PhantomData<()>,
    pub artemis: std::sync::Arc<str>,
}

#[doc = include_str!("1.basic.md")]
pub mod _basic {}
#[doc = include_str!("2.extern-crates.md")]
pub mod _extern_crates {}
#[doc = include_str!("3.lints.md")]
pub mod _lints {}
#[doc = include_str!("4.known-issues.md")]
pub mod _known_issues {}
#[doc = include_str!("5.syntax.md")]
pub mod _syntax {}
#[doc = include_str!("6.disambiguators.md")]
pub mod _disambiguators {}
#[doc = include_str!("7.generics.md")]
pub mod _generics {}
#[doc = include_str!("8.ignored.md")]
pub mod _ignored {}
