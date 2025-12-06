//! Progress reporting and logging for preprocessors.

use std::{
    collections::BTreeSet,
    fmt, io,
    sync::{OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use console::{StyledObject, Term, colors_enabled_stderr, set_colors_enabled};
use env_logger::Logger;
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::{Level, LevelFilter, Log};
use tap::{Pipe, Tap};

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

        if let Some(Spinner { tx, .. }) = SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }

        self
    }

    pub fn update<D: fmt::Display>(&self, prefix: &str, update: D) -> &Self {
        let key = prefix.into();
        let update = update.to_string();
        let msg = Message::Update { key, update };

        if let Some(Spinner { tx, .. }) = SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }

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

        if let Some(Spinner { tx, .. }) = SPINNER.get() {
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

        if let Some(Spinner { tx, .. }) = SPINNER.get() {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }
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

pub fn styled<D>(val: D) -> console::StyledObject<D> {
    if let Some(Spinner { term, .. }) = SPINNER.get() {
        term.style()
    } else {
        console::Style::new().for_stderr()
    }
    .apply_to(val)
}

#[macro_export]
macro_rules! styled {
    ( ( $($display:tt)+ ) . $($style:tt)+ ) => {{
        $crate::logging::styled( $($display)* ) . $($style)*
    }};
}

pub fn is_logging() -> bool {
    SPINNER.get().is_none()
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

/// Either a [`console::Term`] or an [`env_logger::Logger`].
///
/// This is automatically detected upon installation as the global logger. The logic is:
///
/// - If the `RUST_LOG` env var is set, this will use [`env_logger`].
/// - If stderr is not "user-attended", as determined by [`console::user_attended_stderr()`],
///   like if stderr is piped to a file, this will use [`env_logger`].
/// - Otherwise, this will use [`console`].
///
/// When this is a [`console::Term`], logs are handled by the global [`indicatif`] spinner.
///
/// When this is an [`env_logger::Logger`], there will not be a spinner, and progress
/// reports are printed as logs instead.
pub enum ConsoleLogger {
    Console(Term),
    Logger(Logger),
}

impl ConsoleLogger {
    /// Install a [`ConsoleLogger`] as the global [`log`] logger.
    pub fn install(name: &str) {
        Self::try_install(name).expect("logger should not have been set");
    }

    pub fn try_install(name: &str) -> Result<()> {
        log::set_boxed_logger(Box::new(Self::new(name)))?;
        log::set_max_level(LevelFilter::max());
        Ok(())
    }

    fn new(name: &str) -> Self {
        match maybe_logging() {
            Some(LevelFilter::Off) => SPINNER
                .get_or_init(|| spawn_spinner(name))
                .term
                .clone()
                .pipe(Self::Console),
            level => env_logger::Builder::new()
                .format(log_format)
                .parse_default_env()
                .tap_mut(|builder| {
                    if let Some(level) = level {
                        builder.filter_level(level);
                    }
                })
                .build()
                .pipe(Self::Logger),
        }
    }
}

fn maybe_logging() -> Option<LevelFilter> {
    if std::env::var("RUST_LOG")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        // RUST_LOG to be parsed by env_logger
        None
    } else if !console::user_attended_stderr() {
        // RUST_LOG not set but stderr isn't a terminal
        // log info and above
        Some(LevelFilter::Info)
    } else {
        // use spinner instead
        Some(LevelFilter::Off)
    }
}

impl Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        match self {
            ConsoleLogger::Logger(logger) => logger.enabled(metadata),
            ConsoleLogger::Console(_) => {
                if metadata.target().starts_with(env!("CARGO_CRATE_NAME")) {
                    metadata.level() <= Level::Info
                } else {
                    metadata.level() <= Level::Warn
                }
            }
        }
    }

    fn log(&self, record: &log::Record) {
        match self {
            ConsoleLogger::Logger(logger) => logger.log(record),
            ConsoleLogger::Console(term) => {
                if !self.enabled(record.metadata()) {
                    return;
                }
                let Ok(message) = Vec::<u8>::new()
                    .pipe(|mut buf| log_format(&mut buf, record).and(Ok(buf)))
                    .context("failed to emit log message")
                    .and_then(|buf| Ok(String::from_utf8(buf)?))
                else {
                    return;
                };
                let message = styled_log(message.trim_end(), record);
                term.write_line(&message.to_string()).ok();
            }
        }
    }

    fn flush(&self) {
        match self {
            ConsoleLogger::Console(term) => {
                term.flush().ok();
            }
            ConsoleLogger::Logger(logger) => {
                logger.flush();
            }
        }
    }
}

pub static SPINNER: OnceLock<Spinner> = OnceLock::new();

pub struct Spinner {
    tx: mpsc::Sender<Message>,
    term: Term,
}

fn spawn_spinner(name: &str) -> Spinner {
    // https://github.com/console-rs/indicatif/issues/698
    set_colors_enabled(colors_enabled_stderr());

    let (tx, rx) = mpsc::channel();

    let term = Term::stderr();

    let target = term.clone();
    let template = format!("{{spinner:.cyan}} [{name}] {{prefix}} ... {{msg}}",);

    // this thread is detached. this is okay in usage because SPINNER.get_or_init
    // guarantees this function is called at most once

    thread::spawn(move || {
        struct Bar {
            prefix: String,
            bar: ProgressBar,
        }

        let mut current: Option<Bar> = None;

        let mut tasks = BTreeSet::<String>::new();
        let mut task_idx = 0;
        let mut interval = Instant::now();

        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}

                Err(mpsc::RecvTimeoutError::Disconnected) => break,

                Ok(Message::Create { prefix, total }) => {
                    if let Some(bar) = current {
                        bar.bar.abandon()
                    }

                    let style = ProgressStyle::with_template(&template)
                        .unwrap()
                        .tick_chars("⠇⠋⠙⠸⠴⠦⠿");

                    let bar = ProgressDrawTarget::term(target.clone(), 20)
                        .pipe(|target| ProgressBar::with_draw_target(total, target))
                        .with_prefix(prefix.clone())
                        .with_style(style);

                    bar.enable_steady_tick(Duration::from_millis(100));

                    current = Some(Bar { prefix, bar });
                }

                Ok(Message::Update { key, update }) => {
                    let Some(Bar {
                        ref bar,
                        ref prefix,
                    }) = current
                    else {
                        continue;
                    };

                    if &key != prefix {
                        continue;
                    }

                    bar.set_message(update);
                    bar.tick();
                }

                Ok(Message::Finish { key, update }) => {
                    let Some(Bar {
                        ref bar,
                        ref prefix,
                    }) = current
                    else {
                        continue;
                    };

                    if &key != prefix {
                        continue;
                    }

                    bar.finish_with_message(update);
                    current = None;
                }

                Ok(Message::Task { key, task }) => {
                    let Some(Bar {
                        ref bar,
                        ref prefix,
                    }) = current
                    else {
                        continue;
                    };

                    if &key != prefix {
                        continue;
                    }

                    if let Some(length) = bar.length() {
                        let counter = styled(format!("({}/{length})", bar.position())).dim();
                        bar.set_prefix(format!("{prefix} {counter}"))
                    }

                    bar.set_message(styled(&task).magenta().to_string());
                    bar.tick();

                    tasks.insert(task);
                    interval = Instant::now();
                }

                Ok(Message::Done { key, task }) => {
                    let Some(Bar {
                        ref bar,
                        ref prefix,
                    }) = current
                    else {
                        continue;
                    };

                    if &key != prefix {
                        continue;
                    }

                    bar.inc(1);

                    if let Some(length) = bar.length() {
                        let counter = styled(format!("({}/{length})", bar.position())).dim();
                        bar.set_prefix(format!("{prefix} {counter}"))
                    }

                    bar.set_message(styled(&task).green().to_string());
                    bar.tick();

                    tasks.insert(task);
                    interval = Instant::now();
                }
            }

            if let Some(Bar {
                ref prefix,
                ref bar,
            }) = current
            {
                let now = Instant::now();

                if now - interval > Duration::from_secs(10) {
                    interval = now;
                    if task_idx >= tasks.len() {
                        task_idx = 0
                    }
                    if let Some(task) = tasks.iter().nth(task_idx) {
                        spinner_log!(warn!(
                            "task {prefix} - {task} has been running for more than {}",
                            HumanDuration(bar.elapsed())
                        ));
                        bar.set_message(styled(task).magenta().to_string());
                        task_idx += 1;
                    }
                }
            }
        }
    });

    Spinner { tx, term }
}

/// <https://github.com/rust-lang/mdBook/blob/07b25cdb643899aeca2307fbab7690fa7eeec36b/src/main.rs#L100-L109>
fn log_format<W: io::Write>(formatter: &mut W, record: &log::Record) -> io::Result<()> {
    let message = format!(
        "{} [{}] ({}): {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        record.level(),
        record.target(),
        record.args()
    );
    let message = styled_log(message, record);
    writeln!(formatter, "{message}",)
}

fn styled_log<D>(message: D, record: &log::Record) -> StyledObject<D> {
    match record.level() {
        Level::Warn => styled(message).yellow(),
        Level::Error => styled(message).red(),
        Level::Info => styled(message),
        _ => styled(message).dim(),
    }
}
