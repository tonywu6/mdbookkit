use std::{
    collections::BTreeSet,
    io,
    sync::{mpsc, OnceLock},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use console::{colors_enabled_stderr, set_colors_enabled, StyledObject, Term};
use env_logger::Logger;
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use log::{Level, LevelFilter, Log};
use tap::Pipe;

use super::{styled, Message};

/// Either a [console::Term] or an [env_logger::Logger].
pub enum ConsoleLogger {
    Console(Term),
    Logger(Logger),
}

impl Default for ConsoleLogger {
    fn default() -> Self {
        if let Some(spinner) = SPINNER.get() {
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
    pub fn install(name: &str) {
        maybe_spinner(name);
        let logger = Box::new(Self::default());
        log::set_boxed_logger(logger).expect("logger should not have been set");
        log::set_max_level(LevelFilter::max());
    }
}

fn maybe_spinner(name: &str) {
    fn rust_log() -> bool {
        std::env::var("RUST_LOG")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    fn attended() -> bool {
        console::user_attended_stderr()
    }

    if rust_log() || !attended() {
        return;
    }

    SPINNER.set(spawn_spinner(name)).ok();
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
    pub tx: mpsc::Sender<Message>,
    pub term: Term,
}

fn spawn_spinner(name: &str) -> Spinner {
    // https://github.com/console-rs/indicatif/issues/698
    set_colors_enabled(colors_enabled_stderr());

    let (tx, rx) = mpsc::channel();

    let term = Term::stderr();

    let target = term.clone();
    let template = format!("{{spinner:.cyan}} [{name}] {{prefix}} ... {{msg}}",);

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
