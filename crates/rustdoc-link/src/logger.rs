use std::{
    collections::BTreeSet,
    fmt, io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use console::{style, Term};
use env_logger::Logger;
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::{Level, LevelFilter, Log};
use once_cell::sync::{Lazy, OnceCell};
use tap::{Pipe, TapFallible};

use crate::preprocessor_name;

#[macro_export]
macro_rules! log_debug {
    () => {
        |err| log::debug!("{err:?}")
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
}

/// Either a [console::Term] or an [env_logger::Logger].
#[derive(Debug)]
pub enum ConsoleLogger {
    Console(Term),
    Logger(Logger),
}

impl Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        match self {
            ConsoleLogger::Logger(logger) => logger.enabled(metadata),
            ConsoleLogger::Console(_) => {
                if is_from_main(metadata.target()) {
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
                    .context("failed to emit log message")
                    .tap_err(log_debug!())
                else {
                    return;
                };
                let message = message.trim_end();
                let message = match record.level() {
                    Level::Warn => style(message).yellow(),
                    Level::Error => style(message).red(),
                    _ => style(message).dim(),
                };
                term.write_line(&message.to_string()).ok();
            }
        }
    }

    fn flush(&self) {
        match self {
            ConsoleLogger::Logger(logger) => logger.flush(),
            ConsoleLogger::Console(_) => {}
        }
    }
}

impl ConsoleLogger {
    pub fn init() {
        let logger = Box::new(Self::default());
        log::set_boxed_logger(logger).expect("logger should not have been set");
        log::set_max_level(LevelFilter::max());
        IS_CONSOLE_LOG.set(()).unwrap();
    }
}

static IS_CONSOLE_LOG: OnceCell<()> = OnceCell::new();

fn is_console_log() -> bool {
    IS_CONSOLE_LOG.get().is_some()
}

impl Default for ConsoleLogger {
    fn default() -> Self {
        if logging_enabled() {
            env_logger::Builder::new()
                .format(log_format)
                .parse_default_env()
                .build()
                .pipe(Self::Logger)
        } else {
            Self::Console(Term::stderr())
        }
    }
}

/// <https://github.com/rust-lang/mdBook/blob/07b25cdb643899aeca2307fbab7690fa7eeec36b/src/main.rs#L100-L109>
fn log_format<W: io::Write>(formatter: &mut W, record: &log::Record) -> io::Result<()> {
    writeln!(
        formatter,
        "{} [{}] ({}): {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        record.level(),
        record.target(),
        record.args()
    )
}

pub fn logging_enabled() -> bool {
    static ENABLED: Lazy<bool> = Lazy::new(get_logging_enabled);
    *ENABLED
}

fn get_logging_enabled() -> bool {
    let ci = std::env::var("CI").map(|v| v == "true").unwrap_or(false);
    let rust_log = std::env::var("RUST_LOG")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    ci || rust_log
}

fn is_from_main(target: &str) -> bool {
    target.starts_with(env!("CARGO_CRATE_NAME"))
}

macro_rules! should_spin {
    ($($retval:tt)*) => {
        if !is_console_log() { return $($retval)* }
    };
}

#[derive(Debug)]
pub struct Spinner {
    tx: mpsc::Sender<Message>,
}

impl Spinner {
    pub fn create(&self, prefix: &str, total: Option<u64>) -> &Self {
        should_spin!(self);
        let prefix = prefix.into();
        self.tx.send(Message::Create { prefix, total }).ok();
        self
    }

    pub fn update<D: fmt::Display>(&self, prefix: &str, update: D) -> &Self {
        should_spin!(self);
        let key = prefix.into();
        let update = update.to_string();
        self.tx.send(Message::Update { key, update }).ok();
        self
    }

    pub fn task<D: fmt::Display>(&self, prefix: &str, task: D) -> SpinnerHandle {
        let tx = self.tx.clone();

        should_spin!(SpinnerHandle { tx, done: None });

        let key = String::from(prefix);
        let task = task.to_string();

        let open = Message::Task {
            key: key.clone(),
            task: task.clone(),
        };
        tx.send(open).ok();

        let done = Some(Message::Done { key, task });
        SpinnerHandle { tx, done }
    }

    pub fn finish<D: fmt::Display>(&self, prefix: &str, update: D) {
        should_spin!();
        let key = prefix.into();
        let update = update.to_string();
        self.tx.send(Message::Finish { key, update }).ok();
    }
}

#[must_use]
pub struct SpinnerHandle {
    tx: mpsc::Sender<Message>,
    done: Option<Message>,
}

impl Drop for SpinnerHandle {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            self.tx.send(done).ok();
        }
    }
}

#[inline]
pub fn spinner() -> &'static Spinner {
    static SPINNER: Lazy<Spinner> = Lazy::new(create_spinner);
    &SPINNER
}

#[derive(Debug)]
enum Message {
    Create { prefix: String, total: Option<u64> },
    Update { key: String, update: String },
    Task { key: String, task: String },
    Done { key: String, task: String },
    Finish { key: String, update: String },
}

fn create_spinner() -> Spinner {
    let (tx, rx) = mpsc::channel();

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

                    let bar = ProgressBar::with_draw_target(
                        total,
                        if logging_enabled() {
                            ProgressDrawTarget::hidden()
                        } else {
                            ProgressDrawTarget::stderr()
                        },
                    )
                    .with_prefix(prefix.clone())
                    .with_style(SPINNER_STYLE.clone());

                    if logging_enabled() {
                        log::info!(target: env!("CARGO_CRATE_NAME"), "{prefix}");
                    } else {
                        bar.enable_steady_tick(Duration::from_millis(100));
                    }

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

                    if logging_enabled() {
                        log::info!(
                            target: env!("CARGO_CRATE_NAME"),
                            "{prefix} {update}",
                        );
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

                    if logging_enabled() {
                        log::info!(
                            target: env!("CARGO_CRATE_NAME"),
                            "{prefix} .. {update}",
                        );
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

                    if logging_enabled() {
                        log::info!(
                            target: env!("CARGO_CRATE_NAME"),
                            "{prefix} {task}",
                        );
                    }

                    bar.set_message(style(&task).magenta().to_string());
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

                    if logging_enabled() {
                        log::info!(
                            target: env!("CARGO_CRATE_NAME"),
                            "{prefix} {task} done",
                        );
                    }

                    if let Some(length) = bar.length() {
                        bar.inc(1);
                        bar.set_prefix(format!(
                            "{} {}",
                            prefix,
                            style(format!("({}/{length})", bar.position())).dim()
                        ))
                    }

                    bar.set_message(style(&task).green().to_string());
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

                if now - interval > Duration::from_secs(3) {
                    interval = now;
                    if task_idx >= tasks.len() {
                        task_idx = 0
                    }
                    if let Some(task) = tasks.iter().nth(task_idx) {
                        if logging_enabled() {
                            log::warn!(
                                target: env!("CARGO_CRATE_NAME"),
                                "task {prefix} - {task} has been running for {}",
                                HumanDuration(bar.elapsed())
                            )
                        }
                        bar.set_message(style(task).magenta().to_string());
                        task_idx += 1;
                    }
                }
            }
        }
    });

    Spinner { tx }
}

static SPINNER_STYLE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template(&format!(
        "{{spinner:.cyan}} [{}] {{prefix}} ... {{msg}}",
        preprocessor_name()
    ))
    .unwrap()
    .tick_chars("⠇⠋⠙⠸⠴⠦⠿")
});
