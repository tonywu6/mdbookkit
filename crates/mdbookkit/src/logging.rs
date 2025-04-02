//! Progress reporting and logging for preprocessors.

use std::{fmt, sync::mpsc};

pub fn spinner() -> SpinnerHandle {
    SpinnerHandle
}

pub struct SpinnerHandle;

macro_rules! spinner_log {
    ( $level:ident ! ( $($args:tt)* ) ) => {
        log::$level!(target: env!("CARGO_CRATE_NAME"), $($args)*);
    };
}

impl SpinnerHandle {
    pub fn create(&self, prefix: &str, total: Option<u64>) -> &Self {
        let prefix = prefix.into();
        let msg = Message::Create { prefix, total };

        #[cfg(feature = "common-logger")]
        if let Some(terminal::Spinner { tx, .. }) = terminal::SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }

        #[cfg(not(feature = "common-logger"))]
        spinner_log!(info!("{msg}"));

        self
    }

    pub fn update<D: fmt::Display>(&self, prefix: &str, update: D) -> &Self {
        let key = prefix.into();
        let update = update.to_string();
        let msg = Message::Update { key, update };

        #[cfg(feature = "common-logger")]
        if let Some(terminal::Spinner { tx, .. }) = terminal::SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }

        #[cfg(not(feature = "common-logger"))]
        spinner_log!(info!("{msg}"));

        self
    }

    pub fn task<D: fmt::Display>(&self, prefix: &str, task: D) -> TaskHandle {
        let key = String::from(prefix);
        let task = task.to_string();

        let open = Message::Task {
            key: key.clone(),
            task: task.clone(),
        };
        let done = Some(Message::Done { key, task });

        #[cfg(feature = "common-logger")]
        if let Some(terminal::Spinner { tx, .. }) = terminal::SPINNER.get() {
            tx.send(open).ok();
            let spin = Some(tx.clone());
            return TaskHandle { spin, done };
        }

        spinner_log!(info!("{open}"));
        let spin = None;
        TaskHandle { spin, done }
    }

    pub fn finish<D: fmt::Display>(&self, prefix: &str, update: D) {
        let key = prefix.into();
        let update = update.to_string();
        let msg = Message::Finish { key, update };

        #[cfg(feature = "common-logger")]
        if let Some(terminal::Spinner { tx, .. }) = terminal::SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }

        #[cfg(not(feature = "common-logger"))]
        spinner_log!(info!("{msg}"));
    }
}

#[must_use]
pub struct TaskHandle {
    spin: Option<mpsc::Sender<Message>>,
    done: Option<Message>,
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        let Some(done) = self.done.take() else { return };
        if let Some(ref tx) = self.spin {
            tx.send(done).ok();
        } else {
            spinner_log!(info!("{done}"));
        }
    }
}

#[derive(Debug)]
#[cfg_attr(not(feature = "common-logger"), allow(unused))]
enum Message {
    Create { prefix: String, total: Option<u64> },
    Update { key: String, update: String },
    Task { key: String, task: String },
    Done { key: String, task: String },
    Finish { key: String, update: String },
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { prefix, .. } => fmt::Display::fmt(prefix, f),
            Self::Update { key, update } => {
                fmt::Display::fmt(key, f)?;
                fmt::Display::fmt(" ", f)?;
                fmt::Display::fmt(update, f)?;
                Ok(())
            }
            Self::Task { key, task } => {
                fmt::Display::fmt(key, f)?;
                fmt::Display::fmt(" ", f)?;
                fmt::Display::fmt(task, f)?;
                Ok(())
            }
            Self::Done { key, task } => {
                fmt::Display::fmt(key, f)?;
                fmt::Display::fmt(" ", f)?;
                fmt::Display::fmt(task, f)?;
                fmt::Display::fmt(" .. done", f)?;
                Ok(())
            }
            Self::Finish { key, update } => {
                fmt::Display::fmt(key, f)?;
                fmt::Display::fmt(" .. ", f)?;
                fmt::Display::fmt(update, f)?;
                Ok(())
            }
        }
    }
}

#[cfg(feature = "common-logger")]
pub fn styled<D>(val: D) -> console::StyledObject<D> {
    if let Some(terminal::Spinner { term, .. }) = terminal::SPINNER.get() {
        term.style()
    } else {
        console::Style::new().for_stderr()
    }
    .apply_to(val)
}

#[macro_export]
macro_rules! styled {
    ( ( $($display:tt)+ ) . $($style:tt)+ ) => {{
        #[cfg(feature = "common-logger")]
        {
            $crate::logging::styled( $($display)* ) . $($style)*
        }
        #[cfg(not(feature = "common-logger"))]
        {
            $($display)*
        }
    }};
}

#[cfg(feature = "common-logger")]
pub fn is_logging() -> bool {
    terminal::SPINNER.get().is_none()
}

#[macro_export]
macro_rules! log_debug {
    () => {
        |err| log::debug!("{err:?}")
    };
}

#[macro_export]
macro_rules! log_trace {
    () => {
        |err| log::trace!("{err:?}")
    };
}

#[macro_export]
macro_rules! log_warning {
    () => {
        |err| {
            if log::log_enabled!(log::Level::Debug) {
                log::warn!("{err:?}")
            } else {
                log::warn!("{err}")
            }
        }
    };
    (detailed) => {
        |err| log::warn!("{err:?}")
    };
}

#[cfg(feature = "common-logger")]
mod terminal;

#[cfg(feature = "common-logger")]
pub use self::terminal::ConsoleLogger;
