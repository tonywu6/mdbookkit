use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use console::{Style, Term};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use tap::{Pipe, Tap};

use super::{ProgressTick, TickerData, styled};

#[derive(Debug, Clone)]
pub struct MultiProgressTicker {
    tx: Option<mpsc::Sender<ProgressTick>>,
    wr: MultiProgressWriter,
}

impl MultiProgressTicker {
    pub fn new(wr: MultiProgressWriter) -> Self {
        Self { tx: None, wr }.tap_mut(spawn_ticker)
    }

    #[inline]
    pub fn sender(&self) -> Option<mpsc::Sender<ProgressTick>> {
        self.tx.clone()
    }

    #[inline]
    pub fn writer(&self) -> impl Write {
        self.wr.clone()
    }

    #[inline]
    pub fn style(&self) -> Style {
        self.wr.term.style()
    }

    pub fn is_enabled(&self) -> bool {
        self.wr.bars.is_some()
    }
}

fn spawn_ticker(this: &mut MultiProgressTicker) {
    let Some(manager) = this.wr.bars.clone() else {
        return;
    };

    let (tx, rx) = mpsc::channel();
    this.tx = Some(tx);

    let target = this.wr.term.clone();

    thread::spawn(move || {
        struct Bar {
            bar: ProgressBar,
            ticker: TickerData,
            items: BTreeSet<Arc<str>>,
            item_idx: usize,
            interval: Instant,
        }

        impl Bar {
            fn update_count(&self) {
                let Self { ticker, bar, .. } = self;
                if let Some(length) = bar.length() {
                    let counter = styled(format!("({}/{length})", bar.position())).dim();
                    bar.set_prefix(format!("{ticker} {counter}"))
                }
            }
        }

        let style = ProgressStyle::with_template("{spinner:.cyan} {prefix} ... {msg}")
            .unwrap()
            .tick_chars("⠇⠋⠙⠸⠴⠦⠿");

        let mut current = HashMap::new();

        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}

                Err(mpsc::RecvTimeoutError::Disconnected) => break,

                Ok(ProgressTick::TickerCreate(ticker)) => {
                    let bar = ProgressDrawTarget::term(target.clone(), 20)
                        .pipe(|target| ProgressBar::with_draw_target(ticker.count, target))
                        .with_prefix(ticker.to_string())
                        .with_style(style.clone());

                    bar.enable_steady_tick(Duration::from_millis(100));

                    let key = ticker.key;
                    let bar = Bar {
                        bar,
                        ticker,
                        items: Default::default(),
                        item_idx: Default::default(),
                        interval: Instant::now(),
                    };

                    manager.add(bar.bar.clone());
                    current.insert(key, bar);
                }

                Ok(ProgressTick::TickerUpdate { key, msg }) => {
                    let Some(Bar { bar, .. }) = current.get(key) else {
                        continue;
                    };

                    bar.set_message(msg);
                }

                Ok(ProgressTick::ItemOpen { key, item }) => {
                    let Some(current) = current.get_mut(key) else {
                        continue;
                    };

                    current.update_count();
                    current.bar.set_message(styled(&item).magenta().to_string());

                    current.items.insert(item);
                    current.interval = Instant::now();
                }

                Ok(ProgressTick::ItemDone { key, item }) => {
                    let Some(current) = current.get_mut(key) else {
                        continue;
                    };

                    current.bar.inc(1);

                    current.update_count();
                    current.bar.set_message(styled(&item).green().to_string());

                    current.items.remove(&item);
                    current.interval = Instant::now();
                }

                Ok(ProgressTick::TickerFinish { key }) => {
                    let Some(Bar { bar, .. }) = current.remove(key) else {
                        continue;
                    };

                    bar.finish_and_clear();
                    manager.remove(&bar);
                }
            }

            for Bar {
                bar,
                items,
                item_idx,
                interval,
                ..
            } in current.values_mut()
            {
                let now = Instant::now();

                if now - *interval > Duration::from_secs(2) {
                    *interval = now;

                    if let Some(item) = items.iter().nth(*item_idx) {
                        bar.set_message(styled(item).magenta().to_string());
                    }

                    *item_idx += 1;
                    if *item_idx >= items.len() {
                        *item_idx = 0
                    }
                }
            }
        }
    });
}

#[derive(Debug, Clone)]
pub struct MultiProgressWriter {
    bars: Option<MultiProgress>,
    term: Term,
}

// Prevent progress bars from clobbering log output.
// See https://github.com/emersonford/tracing-indicatif/blob/main/src/writer.rs

impl Write for MultiProgressWriter {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.suspended(|term| term.write(buf))
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.suspended(|term| term.flush())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        self.suspended(|term| term.write_vectored(bufs))
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.suspended(|term| term.write_all(buf))
    }

    #[inline]
    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        self.suspended(|term| term.write_fmt(args))
    }
}

impl MultiProgressWriter {
    pub fn new(enabled: bool) -> Self {
        let term = Term::stderr();

        let bars = if enabled {
            ProgressDrawTarget::term(term.clone(), 20)
                .pipe(MultiProgress::with_draw_target)
                .pipe(Some)
        } else {
            None
        };

        Self { bars, term }
    }

    #[inline(always)]
    fn suspended<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut Term) -> T,
    {
        if let Some(ref bars) = self.bars {
            bars.suspend(|| f(&mut self.term))
        } else {
            f(&mut self.term)
        }
    }
}
