use std::{
    collections::BTreeSet,
    fmt::Debug,
    sync::{Arc, LazyLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use console::{StyledObject, Term};
use indicatif::{HumanDuration, ProgressBar, ProgressDrawTarget, ProgressStyle};
use tap::{Pipe, Tap};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
    warn,
};
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::{LevelFilter, filter_fn},
    layer::{Context, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    util::SubscriberInitExt,
};

use crate::env::{MDBOOK_LOG, is_colored, is_logging, set_colored, set_logging};

#[doc(hidden)]
#[macro_export]
macro_rules! branded {
    // cannot use env!("CARGO_PKG_NAME") because that would be
    // the package name at callsite
    ( $suffix:literal ) => {{ concat!("mdbookkit.", $suffix) }};
}

macro_rules! is_branded {
    ( $metadata:expr, $suffix:literal ) => {{ $metadata.fields().field(branded!($suffix)).is_some() }};
    ( $metadata:expr ) => {{
        $metadata
            .fields()
            .iter()
            .any(|f| f.name().starts_with("mdbookkit."))
    }};
}

pub struct Logging {
    pub logging: Option<bool>,
    pub colored: Option<bool>,
    pub level: LevelFilter,
}

impl Logging {
    pub fn init(self) {
        init_logging(self);
    }
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            logging: None,
            colored: None,
            level: LevelFilter::INFO,
        }
    }
}

fn init_logging(options: Logging) {
    if let Some(logging) = options.logging {
        set_logging(logging);
    }
    if let Some(colored) = options.colored {
        set_colored(colored);
    }

    // https://github.com/rust-lang/mdBook/blob/v0.5.2/src/main.rs#L93

    let filter = EnvFilter::builder()
        .with_default_directive(options.level.into())
        .parse_lossy(MDBOOK_LOG.as_deref().unwrap_or_default());

    let logger = tracing_subscriber::fmt::layer()
        .without_time()
        .with_target(filter.max_level_hint().unwrap_or(options.level) < LevelFilter::INFO)
        .with_ansi(is_colored())
        .with_writer(|| WRITER.clone())
        .with_filter(if TICKER.is_some() {
            Some(filter_fn(|metadata| !is_branded!(metadata)))
        } else {
            None
        });

    let ticker = ConsoleLayer.with_filter(if TICKER.is_none() {
        Some(filter_fn(|metadata| !metadata.is_event()))
    } else {
        None
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(logger)
        .with(ticker)
        .init();
}

macro_rules! derive_event {
    ( $id:expr, $metadata:expr, $($field:literal = $value:expr),* ) => {{
        let metadata = $metadata;
        let fields = ::tracing::field::FieldSet::new(&[$($field),*], metadata.callsite());
        #[allow(unused)]
        let mut iter = fields.iter();
        let values = [$(
            (&iter.next().unwrap(), ::core::option::Option::Some(&$value as &dyn tracing::field::Value)),
        )*];
        Event::child_of($id, metadata, &fields.value_set(&values));
    }};
}

#[macro_export]
macro_rules! timer {
    ( $level:expr, $key:literal, $( $span:tt )* ) => {
        ::tracing::span!(
            $level,
            $key,
            { $crate::branded!("timer") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $level:expr, $key:literal ) => {
        $crate::timer!($level, $key,)
    }
}

#[macro_export]
macro_rules! timer_event {
    ( $parent:expr, $level:expr, $( $span:tt )* ) => {
        ::tracing::event!(
            parent: $parent,
            $level,
            { $crate::branded!("timer.event") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $parent:expr, $level:expr ) => {
        $crate::timer_event!($parent, $level,)
    }
}

#[macro_export]
macro_rules! timer_item {
    ( $parent:expr, $level:expr, $key:literal, $( $span:tt )* ) => {
        ::tracing::span!(
            parent: $parent,
            $level,
            $key,
            { $crate::branded!("timer.item") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $parent:expr, $level:expr, $key:literal ) => {
        $crate::timer_item!($parent, $level, $key,)
    }
}

struct ConsoleLayer;

impl<S> Layer<S> for ConsoleLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };

        if is_branded!(span, "timer") {
            let TimerVisitor { count, .. } = TimerVisitor::from_attrs(attrs);

            let timer = Timer {
                prefix: SpanPath::to_string(&span),
                key: span.name(),
                count,
            };

            span.extensions_mut().insert(timer.clone());

            if let Some(ticker) = &*TICKER {
                (ticker.tx).send(ProgressTick::TimerCreate(timer)).ok();
            } else {
                derive_event!(id, span.metadata(), "message" = "started");
            }
        } else if is_branded!(span, "timer.item")
            && let TimerVisitor {
                item: Some(item), ..
            } = TimerVisitor::from_attrs(attrs)
            && let Some(parent) = span.parent()
            && let Some(Timer { key, .. }) = parent.extensions().get::<Timer>()
        {
            span.extensions_mut().insert(TimerItem(item.clone()));

            if let Some(ticker) = &*TICKER {
                (ticker.tx).send(ProgressTick::ItemOpen { key, item }).ok();
            } else {
                derive_event!(id, span.metadata(), "message" = "started");
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };

        // n.b. using extensions_mut involves a RwLock write, which will cause
        // derive_event to deadlock when it tries to read from the same span

        if let Some(Timer { key, .. }) = span.extensions().get::<Timer>() {
            if let Some(ticker) = &*TICKER {
                ticker.tx.send(ProgressTick::TimerFinish { key }).ok();
            } else {
                derive_event!(id, span.metadata(), "message" = "finished");
            }
        } else if let Some(parent) = span.parent()
            && let Some(Timer { key, .. }) = parent.extensions().get::<Timer>()
            && let Some(TimerItem(item)) = span.extensions().get::<TimerItem>()
        {
            if let Some(ticker) = &*TICKER {
                let item = item.clone();
                (ticker.tx).send(ProgressTick::ItemDone { key, item }).ok();
            } else {
                derive_event!(id, span.metadata(), "message" = "finished");
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !is_branded!(event.metadata(), "timer.event") {
            return;
        }

        if let TimerVisitor {
            message: Some(msg), ..
        } = TimerVisitor::from_event(event)
            && let Some(span) = (event.parent().cloned())
                .or_else(|| ctx.current_span().id().cloned())
                .and_then(|id| ctx.span(&id))
            && let Some(Timer { key, .. }) = span.extensions().get::<Timer>()
        {
            if let Some(ticker) = &*TICKER {
                (ticker.tx)
                    .send(ProgressTick::TimerUpdate { key, msg })
                    .ok();
            }
        }
    }
}

#[derive(Debug, Default)]
struct TimerVisitor {
    message: Option<String>,
    item: Option<Arc<str>>,
    count: Option<u64>,
}

impl Visit for TimerVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.into()),
            "item" => self.item = Some(value.into()),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "count" {
            self.count = Some(value)
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.record_str(field, &format!("{value:?}"));
    }
}

impl TimerVisitor {
    #[inline]
    fn from_attrs(attrs: &Attributes<'_>) -> Self {
        Self::default().tap_mut(|v| attrs.values().record(v))
    }

    #[inline]
    fn from_event(event: &Event<'_>) -> Self {
        Self::default().tap_mut(|v| event.record(v))
    }
}

struct SpanPath<'a, R: LookupSpan<'a>>(Option<SpanRef<'a, R>>);

impl<'a, R: LookupSpan<'a>> Iterator for SpanPath<'a, R> {
    type Item = &'static str;

    fn next(&mut self) -> Option<Self::Item> {
        let span = self.0.take()?;
        self.0 = span.parent();
        Some(span.name())
    }
}

impl<'a, R: LookupSpan<'a>> SpanPath<'a, R> {
    fn new(span: &SpanRef<'a, R>) -> Self {
        Self(span.parent())
    }

    fn to_string(span: &SpanRef<'a, R>) -> Option<Arc<str>> {
        let mut items = Self::new(span).collect::<Vec<_>>();
        if items.is_empty() {
            None
        } else {
            items.reverse();
            Some(items.join(":").into())
        }
    }
}

#[derive(Debug, Clone)]
struct Timer {
    prefix: Option<Arc<str>>,
    key: &'static str,
    count: Option<u64>,
}

impl std::fmt::Display for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref prefix) = self.prefix {
            write!(f, "[{prefix}] {}", self.key)
        } else {
            write!(f, "{}", self.key)
        }
    }
}

#[derive(Debug, Clone)]
struct TimerItem(Arc<str>);

#[derive(Debug)]
enum ProgressTick {
    TimerCreate(Timer),
    TimerUpdate { key: &'static str, msg: String },
    ItemOpen { key: &'static str, item: Arc<str> },
    ItemDone { key: &'static str, item: Arc<str> },
    TimerFinish { key: &'static str },
}

#[derive(Clone)]
struct ProgressTicker {
    tx: mpsc::Sender<ProgressTick>,
}

static WRITER: LazyLock<Term> = LazyLock::new(Term::stderr);

static TICKER: LazyLock<Option<ProgressTicker>> = LazyLock::new(|| {
    if is_logging() {
        None
    } else {
        Some(spawn_ticker())
    }
});

fn spawn_ticker() -> ProgressTicker {
    let (tx, rx) = mpsc::channel();

    let style = ProgressStyle::with_template("{spinner:.cyan} {prefix} ... {msg}")
        .unwrap()
        .tick_chars("⠇⠋⠙⠸⠴⠦⠿");

    thread::spawn(move || {
        struct Bar {
            timer: Timer,
            bar: ProgressBar,
        }

        impl Bar {
            fn count(&self) {
                let Self { timer, bar } = self;
                if let Some(length) = bar.length() {
                    let counter = styled(format!("({}/{length})", bar.position())).dim();
                    bar.set_prefix(format!("{timer} {counter}"))
                }
            }
        }

        let mut current: Option<Bar> = None;

        let mut tasks = BTreeSet::new();
        let mut task_idx = 0;
        let mut interval = Instant::now();

        loop {
            let current_ref = |key: &str| {
                let bar = current.as_ref()?;
                if bar.timer.key == key {
                    Some(bar)
                } else {
                    None
                }
            };

            match rx.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}

                Err(mpsc::RecvTimeoutError::Disconnected) => break,

                Ok(ProgressTick::TimerCreate(timer)) => {
                    if let Some(bar) = current {
                        bar.bar.abandon()
                    }

                    let bar = ProgressDrawTarget::term(WRITER.clone(), 20)
                        .pipe(|target| ProgressBar::with_draw_target(timer.count, target))
                        .with_prefix(timer.to_string())
                        .with_style(style.clone());

                    bar.enable_steady_tick(Duration::from_millis(100));

                    current = Some(Bar { timer, bar });
                }

                Ok(ProgressTick::TimerUpdate { key, msg }) => {
                    let Some(Bar { bar, .. }) = current_ref(key) else {
                        continue;
                    };

                    bar.set_message(msg);
                    bar.tick();
                }

                Ok(ProgressTick::TimerFinish { key }) => {
                    let Some(Bar { bar, .. }) = current_ref(key) else {
                        continue;
                    };

                    bar.finish_with_message(styled("done").green().to_string());
                    current = None;
                }

                Ok(ProgressTick::ItemOpen { key, item }) => {
                    let Some(current) = current_ref(key) else {
                        continue;
                    };

                    current.count();
                    current.bar.set_message(styled(&item).magenta().to_string());
                    current.bar.tick();

                    tasks.insert(item);
                    interval = Instant::now();
                }

                Ok(ProgressTick::ItemDone { key, item }) => {
                    let Some(current) = current_ref(key) else {
                        continue;
                    };

                    current.bar.inc(1);

                    current.count();
                    current.bar.set_message(styled(&item).green().to_string());
                    current.bar.tick();

                    tasks.remove(&item);
                    interval = Instant::now();
                }
            }

            if let Some(Bar {
                timer: Timer { key, .. },
                ref bar,
                ..
            }) = current
            {
                let now = Instant::now();

                if now - interval > Duration::from_secs(10) {
                    interval = now;

                    if task_idx >= tasks.len() {
                        task_idx = 0
                    }

                    if let Some(task) = tasks.iter().nth(task_idx) {
                        let elapsed = HumanDuration(bar.elapsed());
                        // TODO: how to attach to current span
                        // TODO: how to clear line for tracing when ticker is running
                        warn!("task {key} - {task} has been running for more than {elapsed}");
                        bar.set_message(styled(task).magenta().to_string());
                        task_idx += 1;
                    }
                }
            }
        }
    });

    ProgressTicker { tx }
}

#[inline]
pub fn stderr() -> impl std::io::Write {
    WRITER.clone()
}

// FIXME: ensure colors do not appear in tracing output
// https://github.com/tokio-rs/tracing/issues/3378
#[inline]
pub fn styled<D>(val: D) -> StyledObject<D> {
    WRITER.style().apply_to(val)
}

#[macro_export]
macro_rules! emit_trace {
    () => {
        |err| ::tracing::trace!("{err:?}")
    };
    ($fmt:expr) => {
        |e| ::tracing::trace!($fmt, e)
    };
}

#[macro_export]
macro_rules! emit_debug {
    () => {
        |err| ::tracing::debug!("{err:?}")
    };
    ($fmt:expr) => {
        |e| ::tracing::debug!($fmt, e)
    };
}

#[macro_export]
macro_rules! emit_warning {
    () => {
        |e| {
            if ::tracing::enabled!(::tracing::Level::DEBUG) {
                ::tracing::warn!("{:?}", e)
            } else {
                ::tracing::warn!("{}", e)
            }
        }
    };
    ($fmt:expr) => {
        |e| ::tracing::warn!($fmt, e)
    };
}

// TODO: clean up logging messages & make use of spans/instrument
