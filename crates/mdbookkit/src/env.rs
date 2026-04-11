use std::{io::IsTerminal, ops::Deref, sync::LazyLock};

use console::colors_enabled_stderr;

macro_rules! env_var {
    ($name:ident $(, $extra:ident)*) => {
        pub(crate) static $name: LazyLock<Option<String>> =
            LazyLock::new(|| {
                std::env::var(stringify!($name))
                    $( .or_else(|_| std::env::var(stringify!($extra))) )*
                    .ok()
            });
    };
}

env_var!(MDBOOK_LOG, RUST_LOG);

env_var!(CI);
env_var!(FORCE_COLOR);
env_var!(NO_COLOR);

env_var!(MDBOOKKIT_TERM_PROGRESS);
env_var!(MDBOOKKIT_TERM_GRAPHICAL);

#[inline]
pub fn is_ci() -> Option<&'static str> {
    CI.truthy()
}

#[inline]
pub fn is_logging() -> bool {
    if MDBOOKKIT_TERM_PROGRESS.truthy().is_none() {
        MDBOOK_LOG.is_some() || is_ci().is_some() || !std::io::stderr().is_terminal()
    } else {
        false
    }
}

#[inline]
pub fn is_colored() -> bool {
    static IS_COLORED: LazyLock<bool> = LazyLock::new(|| {
        if FORCE_COLOR.truthy().is_some() {
            true
        } else if NO_COLOR.truthy().is_some() {
            false
        } else {
            colors_enabled_stderr()
        }
    });
    *IS_COLORED
}

pub trait TruthyStr {
    fn truthy(&self) -> Option<&str>;
}

impl<S: Deref<Target = str>> TruthyStr for Option<S> {
    fn truthy(&self) -> Option<&str> {
        let text = self.as_deref();
        if matches!(text, None | Some("")) {
            None
        } else {
            text
        }
    }
}
