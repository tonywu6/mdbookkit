use std::{
    collections::BTreeSet,
    fmt, io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use console::{colors_enabled_stderr, set_colors_enabled, StyledObject, Term};
use env_logger::Logger;
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::{Level, LevelFilter, Log};
use once_cell::sync::Lazy;
use tap::Pipe;

use crate::preprocessor_name;

/// Either a [console::Term] or an [env_logger::Logger].
pub enum ConsoleLogger {
    Console(Term),
    Logger(Logger),
}

impl Default for ConsoleLogger {
    fn default() -> Self {
        if let Some(spinner) = &*SPINNER {
            Self::Console(spinner.term.clone())
        } else {
            env_logger::Builder::new()
                .format(log_format)
                .parse_default_env()
                .build()
                .pipe(Self::Logger)
        }
    }
}

impl ConsoleLogger {
    pub fn install() {
        let logger = Box::new(Self::default());
        log::set_boxed_logger(logger).expect("logger should not have been set");
        log::set_max_level(LevelFilter::max());
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
                let message = message.trim_end();
                let message = match record.level() {
                    Level::Warn => styled(message).yellow(),
                    Level::Error => styled(message).red(),
                    _ => styled(message).dim(),
                };
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

pub fn spinner() -> SpinnerHandle {
    SpinnerHandle
}

macro_rules! spinner_log {
    ( $level:ident ! ( $($args:tt)* ) ) => {
        log::$level!(target: env!("CARGO_CRATE_NAME"), $($args)*);
    };
}

pub struct SpinnerHandle;

impl SpinnerHandle {
    pub fn create(&self, prefix: &str, total: Option<u64>) -> &Self {
        let prefix = prefix.into();
        let msg = Message::Create { prefix, total };
        if let Some(Spinner { tx, .. }) = &*SPINNER {
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
        if let Some(Spinner { tx, .. }) = &*SPINNER {
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

        if let Some(Spinner { tx, .. }) = &*SPINNER {
            tx.send(open).ok();
            let spin = Some(tx.clone());
            TaskHandle { spin, done }
        } else {
            spinner_log!(info!("{open}"));
            let spin = None;
            TaskHandle { spin, done }
        }
    }

    pub fn finish<D: fmt::Display>(&self, prefix: &str, update: D) {
        let key = prefix.into();
        let update = update.to_string();
        let msg = Message::Finish { key, update };
        if let Some(Spinner { tx, .. }) = &*SPINNER {
            tx.send(msg).ok();
        } else {
            spinner_log!(info!("{msg}"));
        }
    }
}

static SPINNER: Lazy<Option<Spinner>> = Lazy::new(|| {
    fn rust_log() -> bool {
        std::env::var("RUST_LOG")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    fn attended() -> bool {
        console::user_attended_stderr()
    }

    if rust_log() || !attended() {
        None
    } else {
        Some(spawn_spinner())
    }
});

struct Spinner {
    tx: mpsc::Sender<Message>,
    term: Term,
}

#[derive(Debug)]
enum Message {
    Create { prefix: String, total: Option<u64> },
    Update { key: String, update: String },
    Task { key: String, task: String },
    Done { key: String, task: String },
    Finish { key: String, update: String },
}

fn spawn_spinner() -> Spinner {
    // https://github.com/console-rs/indicatif/issues/TODO:
    set_colors_enabled(colors_enabled_stderr());

    let (tx, rx) = mpsc::channel();

    let term = Term::buffered_stderr();

    let target = term.clone();

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

                    let style = ProgressStyle::with_template(&format!(
                        "{{spinner:.cyan}} [{}] {{prefix}} ... {{msg}}",
                        preprocessor_name()
                    ))
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

                    if let Some(length) = bar.length() {
                        bar.inc(1);
                        bar.set_prefix(format!(
                            "{} {}",
                            prefix,
                            styled(format!("({}/{length})", bar.position())).dim()
                        ))
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

                if now - interval > Duration::from_secs(3) {
                    interval = now;
                    if task_idx >= tasks.len() {
                        task_idx = 0
                    }
                    if let Some(task) = tasks.iter().nth(task_idx) {
                        spinner_log!(warn!(
                            "task {prefix} - {task} has been running for {}",
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

pub fn styled<D>(val: D) -> StyledObject<D> {
    if let Some(Spinner { term, .. }) = &*SPINNER {
        term.style()
    } else {
        console::Style::new().for_stderr()
    }
    .apply_to(val)
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
}
