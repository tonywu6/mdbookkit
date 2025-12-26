use std::{
    io::IsTerminal,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use console::{colors_enabled_stderr, set_colors_enabled_stderr};

static CI: LazyLock<String> = LazyLock::new(|| std::env::var("CI").unwrap_or("".into()));

pub(crate) static MDBOOK_LOG: LazyLock<Option<String>> = LazyLock::new(|| {
    std::env::var("MDBOOK_LOG")
        // mdBook v0.4.x
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
});

#[inline]
pub fn is_ci() -> Option<&'static str> {
    let ci = CI.as_str();
    if matches!(ci, "" | "0" | "false") {
        None
    } else {
        Some(ci)
    }
}

static IS_LOGGING: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn is_logging() -> bool {
    if cfg!(feature = "_testing") {
        IS_LOGGING.load(Ordering::Relaxed)
    } else {
        IS_LOGGING.load(Ordering::Relaxed)
            || is_ci().is_some()
            || MDBOOK_LOG.is_some()
            || !std::io::stderr().is_terminal()
    }
}

pub(crate) fn set_logging(enabled: bool) {
    IS_LOGGING.store(enabled, Ordering::Relaxed);
}

#[inline]
pub fn is_colored() -> bool {
    colors_enabled_stderr()
}

#[inline]
pub(crate) fn set_colored(enabled: bool) {
    set_colors_enabled_stderr(enabled);
}
