#[doc(hidden)]
pub fn hidden() {}

pub use crate::internal::exported;

#[doc(hidden)]
pub mod internal {
    pub fn exported() {}
}
