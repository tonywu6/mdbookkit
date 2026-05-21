use std::{
    fmt::{Debug, Display},
    path::Path,
    process::exit,
    sync::{
        LockResult,
        atomic::{AtomicU8, Ordering},
    },
};

use anyhow::{Context, Error, Result, anyhow};
use serde::Deserialize;
use tap::Pipe;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{Layer, layer};

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

pub trait ExpectLock<T> {
    fn expect_lock(self) -> T;
}

impl<T> ExpectLock<T> for LockResult<T> {
    #[inline(always)]
    fn expect_lock(self) -> T {
        self.expect("lock should not be poisoned")
    }
}

#[allow(async_fn_in_trait)]
pub trait FutureWithError<T> {
    async fn context<C>(self, context: C) -> Result<T>
    where
        C: Display + Send + Sync + 'static;

    async fn with_context<C, G>(self, context: G) -> Result<T>
    where
        C: Display + Send + Sync + 'static,
        G: FnOnce() -> C;
}

impl<F, T, E> FutureWithError<T> for F
where
    F: Future<Output = Result<T, E>>,
    E: Into<Error>,
{
    #[inline(always)]
    async fn context<C>(self, context: C) -> Result<T>
    where
        C: Display + Send + Sync + 'static,
    {
        match self.await {
            Ok(value) => Ok(value),
            Err(error) => Err(error.into()).context(context),
        }
    }

    #[inline(always)]
    async fn with_context<C, G>(self, context: G) -> Result<T>
    where
        C: Display + Send + Sync + 'static,
        G: FnOnce() -> C,
    {
        match self.await {
            Ok(value) => Ok(value),
            Err(error) => Err(error.into()).with_context(context),
        }
    }
}

pub trait PathDebug {
    fn debug(&self) -> impl Debug;
}

impl PathDebug for Path {
    #[inline]
    fn debug(&self) -> impl Debug {
        struct DebugPath<'a>(&'a Path);

        impl Debug for DebugPath<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self.0.display().to_string())
            }
        }

        DebugPath(self)
    }
}

pub trait WithPathDebug<T> {
    fn with_path_label(self, path: impl AsRef<Path>, label: &str) -> Result<T>;

    #[inline]
    fn with_path_debug(self, path: impl AsRef<Path>) -> Result<T>
    where
        Self: Sized,
    {
        self.with_path_label(path, "path")
    }
}

impl<T, E> WithPathDebug<T> for Result<T, E>
where
    Result<T, E>: Context<T, E>,
{
    #[inline]
    fn with_path_label(self, path: impl AsRef<Path>, label: &str) -> Result<T> {
        self.with_context(|| format!("{label}: {:?}", path.as_ref().debug()))
    }
}

impl<T> WithPathDebug<T> for Option<T> {
    #[inline]
    fn with_path_label(self, path: impl AsRef<Path>, label: &str) -> Result<T> {
        self.with_context(|| format!("{label}: {:?}", path.as_ref().debug()))
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
        concat!("for more info, see: ", $crate::doc_link!($page))
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
