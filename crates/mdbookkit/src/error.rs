use std::{
    fmt::Debug,
    path::Path,
    process::exit,
    sync::atomic::{AtomicU8, Ordering},
};

use anyhow::{Context, Result, anyhow};
use camino::Utf8Path;
use serde::Deserialize;
use tap::Pipe;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{Layer, layer};
use url::Url;

use crate::env::is_ci;

static MAX_SEVERITY: AtomicU8 = AtomicU8::new(0);

#[inline]
pub fn has_severity(level: Level) -> bool {
    MAX_SEVERITY.load(Ordering::Relaxed) >= level_to_severity(level)
}

#[inline]
pub fn put_severity(level: Level) {
    let severity = level_to_severity(level);
    MAX_SEVERITY.fetch_max(severity, Ordering::Relaxed);
}

pub struct EventLevelLayer;

impl<S: Subscriber> Layer<S> for EventLevelLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: layer::Context<'_, S>) {
        put_severity(*event.metadata().level());
    }
}

#[inline]
fn level_to_severity(level: Level) -> u8 {
    if level <= Level::ERROR {
        50
    } else if level <= Level::WARN {
        40
    } else if level <= Level::INFO {
        30
    } else if level <= Level::DEBUG {
        20
    } else {
        10
    }
}

#[derive(Deserialize, Debug, Default, Clone, Copy)]
pub enum FailOnWarnings {
    #[default]
    #[serde(skip)]
    Unspecified,
    #[serde(rename = "ci")]
    InPipelines,
    #[serde(rename = "always")]
    Always,
}

impl FailOnWarnings {
    #[inline]
    pub fn check(&self) -> Result<()> {
        if has_severity(Level::ERROR) {
            Err(anyhow!("preprocessor finished with errors"))
        } else if has_severity(Level::WARN) {
            match (self, is_ci()) {
                (Self::Always, _) => anyhow! { "treating warnings as errors because the \
                `fail-on-warnings` option is set to \"always\"" }
                .pipe(Err),
                (Self::InPipelines, Some(ci)) => {
                    anyhow! { "treating warnings as errors because CI={ci} and the \
                    `fail-on-warnings` option is set to \"ci\"" }
                    .pipe(Err)
                }
                (Self::Unspecified, Some(ci)) => {
                    anyhow! { "treating warnings as errors because CI={ci} (option \
                    `fail-on-warnings` set to \"ci\" by default)" }
                    .pipe(Err)
                }
                (Self::InPipelines | Self::Unspecified, None) => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    #[inline]
    pub fn adjusted<T, E>(&self, result: Result<Result<T, E>, E>) -> Result<Result<T, E>, E> {
        match result {
            Err(error) => Err(error),
            Ok(Err(error)) => match (self, is_ci()) {
                (Self::Always, _) => Err(error),
                (Self::InPipelines | Self::Unspecified, Some(_)) => Err(error),
                (Self::InPipelines | Self::Unspecified, None) => Ok(Err(error)),
            },
            Ok(Ok(result)) => Ok(Ok(result)),
        }
    }
}

pub trait ExpectFmt {
    fn expect_fmt(self);
}

impl ExpectFmt for std::fmt::Result {
    #[inline(always)]
    fn expect_fmt(self) {
        self.expect("string formatting should not fail")
    }
}

#[macro_export]
macro_rules! write_str {
    ( $($tt:tt)+ ) => {{
        use std::fmt::Write;
        use $crate::error::ExpectFmt;
        write!( $($tt)+ ).expect_fmt();
    }};
}

pub trait Show {
    fn show(&self) -> impl Debug;
}

impl Show for str {
    #[inline]
    fn show(&self) -> impl Debug {
        self
    }
}

impl Show for Url {
    #[inline]
    fn show(&self) -> impl Debug {
        self.as_str()
    }
}

impl Show for Path {
    #[inline]
    fn show(&self) -> impl Debug {
        struct DebugPath<'a>(&'a Path);
        return DebugPath(self);
        impl Debug for DebugPath<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self.0.display().to_string())
            }
        }
    }
}

impl Show for Utf8Path {
    #[inline]
    fn show(&self) -> impl Debug {
        self.as_str()
    }
}

pub trait WithDebugContext<T, E> {
    fn with_debug(self, debug: &(impl Show + ?Sized), label: &'static str) -> Result<T>;

    #[inline]
    fn with_path_debug(self, path: impl AsRef<Path>) -> Result<T>
    where
        Self: Sized,
    {
        self.with_debug(path.as_ref(), "path")
    }
}

impl<C: Context<T, E>, T, E> WithDebugContext<T, E> for C {
    #[inline]
    fn with_debug(self, debug: &(impl Show + ?Sized), label: &'static str) -> Result<T> {
        self.with_context(|| format!("{label}: {:?}", debug.show()))
    }
}

pub trait MapDeserializeError<T, E> {
    fn or_serde_error<E2: serde::de::Error>(self) -> Result<T, E2>;
}

impl<T, E: Into<anyhow::Error>> MapDeserializeError<T, E> for Result<T, E> {
    fn or_serde_error<E2: serde::de::Error>(self) -> Result<T, E2> {
        match self {
            Ok(data) => Ok(data),
            Err(err) => {
                let err = err.into();
                let err = serde::de::Error::custom(format!("{err:?}"));
                Err(err)
            }
        }
    }
}

#[macro_export]
macro_rules! emit_trace {
    ($fmt:literal $($args:tt)*) => {{
        |e| {
            let e = ::anyhow::Error::from(e);
            ::tracing::trace!($fmt, e $($args)*);
            Err(())
        }
    }};
    () => {
        $crate::emit_trace!("{:?}")
    };
}

#[macro_export]
macro_rules! emit_debug {
    ($fmt:literal $($args:tt)*) => {{
        |e| {
            let e = ::anyhow::Error::from(e);
            ::tracing::debug!($fmt, e $($args)*);
            Err(())
        }
    }};
    () => {
        $crate::emit_debug!("{:?}")
    };
}

#[macro_export]
macro_rules! emit_warning {
    ($fmt:literal $($args:tt)*) => {{
        |e| {
            let e = ::anyhow::Error::from(e);
            ::tracing::warn!($fmt, e $($args)*);
            Err(())
        }
    }};
    () => {
        $crate::emit_warning!("{:?}")
    };
}

#[macro_export]
macro_rules! emit_error {
    ($fmt:literal $($args:tt)*) => {{
        |e| {
            let e = ::anyhow::Error::from(e);
            ::tracing::error!($fmt, e $($args)*);
            Err(())
        }
    }};
    () => {
        $crate::emit_error!("{:?}")
    };
}

#[macro_export]
macro_rules! with_bug_report {
    ($emit:ident) => {
        $emit!(
            "{:?}\n\nthis could be a bug; please file a bug report at {repo}/issues",
            repo = env!("CARGO_PKG_REPOSITORY")
        )
    };
}

#[macro_export]
macro_rules! doc_link {
    (help = $page:literal) => {
        concat!("help: for more info, see ", $crate::doc_link!($page))
    };
    (see = $page:literal) => {
        concat!("for more info, see ", $crate::doc_link!($page))
    };
    ($page:literal) => {
        concat!(env!("CARGO_PKG_HOMEPAGE"), "/", $page)
    };
}

pub trait ProgramExit {
    fn exit(self) -> !;
}

impl ProgramExit for Result<(), ()> {
    fn exit(self) -> ! {
        match self {
            Ok(()) => exit(0),
            Err(_) => exit(1),
        }
    }
}

#[macro_export]
macro_rules! try2 {
    ({ $($tt:tt)+ }) => {
        (|| -> ::anyhow::Result<_> { $($tt)+ })()
    };
}
