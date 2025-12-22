//! Logging facilities.
//!
//! ## How the ticker system works
//!
//! The ticker system integrates with [`tracing`].
//!
//! Use [`ticker!`][crate::ticker] to create a progress ticker backed by a [`tracing::Span`].
//! When the span [closes][mod@tracing::span#closing-spans], the ticker is cleared.
//!
//! ```
//! # use tracing::Level;
//! # use mdbookkit::ticker;
//! #
//! let ticker = ticker!(Level::INFO, "task-name", count = 63, "running errands");
//! // ⠋ [parent-span] running errands (0/63) ...
//! ```
//!
//! Use [`ticker_event!`][crate::ticker_event] to flash a message in the specified ticker.
//! The message is backed by a [`tracing::Event`].
//!
//! ```
//! # use tracing::Level;
//! # use mdbookkit::{ticker, ticker_event};
//! # let ticker = ticker!(Level::INFO, "task-name", count = 63, "running errands");
//! #
//! ticker_event!(&ticker, Level::INFO, "task updated");
//! // ⠋ [parent-span] running errands (0/63) ... task updated
//! ```
//!
//! Use [`ticker_item!`][crate::ticker_item] to add a subtask to the specified ticker,
//! backed by a [`tracing::Span`]. When the span closes, the item count in the ticker
//! is increased by 1.
//!
//! ```
//! # use tracing::Level;
//! # use mdbookkit::{ticker, ticker_item};
//! # let ticker = ticker!(Level::INFO, "task-name", count = 63, "running errands");
//! #
//! let item = ticker_item!(&ticker, Level::INFO, "task", "groceries");
//! // ⠋ [parent-span] running errands (0/63) ... groceries
//! drop(item);
//! // ⠋ [parent-span] running errands (1/63) ... groceries
//! ```
//!
//! If the application is configured to be in logging mode, these are emitted as regular logs.
//!
//! ```plaintext
//! INFO parent-span:task-name: started running errands count=63
//! INFO parent-span:task-name: task updated running errands count=63
//! INFO parent-span:task-name:task: started running errands count=63 groceries
//! INFO parent-span:task-name:task: finished running errands count=63 groceries
//! INFO parent-span:task-name: finished running errands count=63
//! ```
//!
//! ### Notes
//!
//! This system uses the parent-child relationship between spans and events to known
//! which progress bars to update.
//!
//! Parent spans must be specified explicitly because ticker and item lifecycles are
//! tracked in [`Layer::on_new_span`] and [`Layer::on_close`], during which a parent span
//! may not have been [entered][mod@tracing::span#entering-a-span], resulting in a
//! `ticker_item!` or a `ticker_event!` without a parent.
//!
//! Spans are tracked at open/close instead of enter/exit because spans may enter/exit
//! multiple times, which does not make sense for progress bars.

use std::{
    fmt::{Debug, Display},
    io::Write,
    sync::{Arc, LazyLock},
};

use console::StyledObject;
use tap::{Pipe, Tap};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
};
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::{LevelFilter, filter_fn},
    layer::{Context, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    util::SubscriberInitExt,
};

use crate::env::{MDBOOK_LOG, is_colored, is_logging, set_colored, set_logging};

use self::writer::{MultiProgressTicker, MultiProgressWriter};

mod writer;

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
        .compact()
        .without_time()
        .with_target(filter.max_level_hint().unwrap_or(options.level) > LevelFilter::INFO)
        .with_ansi(is_colored())
        .with_writer(|| TICKER.writer())
        .with_filter(if TICKER.is_enabled() {
            Some(filter_fn(|metadata| !is_branded!(metadata)))
        } else {
            None
        });

    let ticker = TickerLayer.with_filter(if !TICKER.is_enabled() {
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
    ( $id:expr, $metadata:expr, $format:literal $($vars:tt)* ) => {{
        derive_event!($id, $metadata, "message" = format!($format $($vars)*))
    }};
}

#[macro_export]
macro_rules! ticker {
    ( $level:expr, $key:literal, $( $span:tt )* ) => {
        ::tracing::span!(
            $level,
            $key,
            { $crate::branded!("ticker") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $level:expr, $key:literal ) => {
        $crate::ticker!($level, $key,)
    }
}

#[macro_export]
macro_rules! ticker_event {
    ( $parent:expr, $level:expr, $( $span:tt )* ) => {
        ::tracing::event!(
            parent: $parent,
            $level,
            { $crate::branded!("ticker.event") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $parent:expr, $level:expr ) => {
        $crate::ticker_event!($parent, $level,)
    }
}

#[macro_export]
macro_rules! ticker_item {
    ( $parent:expr, $level:expr, $key:literal, $( $span:tt )* ) => {
        ::tracing::span!(
            parent: $parent,
            $level,
            $key,
            { $crate::branded!("ticker.item") } = ::tracing::field::Empty,
            $($span)*
        )
    };
    ( $parent:expr, $level:expr, $key:literal ) => {
        $crate::ticker_item!($parent, $level, $key,)
    }
}

struct TickerLayer;

impl<S> Layer<S> for TickerLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };

        if is_branded!(span, "ticker") {
            let TickerVisitor { count, message, .. } = TickerVisitor::from_attrs(attrs);

            let ticker = TickerData {
                prefix: SpanPath::to_string(&span),
                key: span.name(),
                title: message,
                count,
            };

            span.extensions_mut().insert(ticker.clone());

            if let Some(tx) = TICKER.sender() {
                tx.send(ProgressTick::TickerCreate(ticker)).ok();
            } else {
                derive_event!(id, span.metadata(), "started");
            }
        } else if is_branded!(span, "ticker.item")
            && let TickerVisitor {
                message: Some(item),
                ..
            } = TickerVisitor::from_attrs(attrs)
            && let Some(parent) = span.parent()
            && let Some(TickerData { key, .. }) = parent.extensions().get::<TickerData>()
        {
            span.extensions_mut().insert(TickerItem(item.clone()));

            if let Some(tx) = TICKER.sender() {
                tx.send(ProgressTick::ItemOpen { key, item }).ok();
            } else {
                derive_event!(id, span.metadata(), "started");
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };

        // n.b. using extensions_mut involves a RwLock write, which will cause
        // derive_event to deadlock when it tries to read from the same span

        if let Some(TickerData { key, .. }) = span.extensions().get::<TickerData>() {
            if let Some(tx) = TICKER.sender() {
                tx.send(ProgressTick::TickerFinish { key }).ok();
            } else {
                derive_event!(id, span.metadata(), "finished");
            }
        } else if let Some(parent) = span.parent()
            && let Some(TickerData { key, .. }) = parent.extensions().get::<TickerData>()
            && let Some(TickerItem(item)) = span.extensions().get::<TickerItem>()
        {
            if let Some(tx) = TICKER.sender() {
                let item = item.clone();
                tx.send(ProgressTick::ItemDone { key, item }).ok();
            } else {
                derive_event!(id, span.metadata(), "finished");
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if !is_branded!(event.metadata(), "ticker.event") {
            return;
        }

        if let TickerVisitor {
            message: Some(msg), ..
        } = TickerVisitor::from_event(event)
            && let Some(span) = (event.parent().cloned())
                .or_else(|| ctx.current_span().id().cloned())
                .and_then(|id| ctx.span(&id))
            && let Some(TickerData { key, .. }) = span.extensions().get::<TickerData>()
        {
            if let Some(tx) = TICKER.sender() {
                tx.send(ProgressTick::TickerUpdate { key, msg }).ok();
            }
        }
    }
}

#[derive(Debug, Default)]
struct TickerVisitor {
    message: Option<Arc<str>>,
    count: Option<u64>,
}

impl Visit for TickerVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.into())
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

impl TickerVisitor {
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
struct TickerData {
    prefix: Option<Arc<str>>,
    key: &'static str,
    title: Option<Arc<str>>,
    count: Option<u64>,
}

impl Display for TickerData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let title = self.title.as_deref().unwrap_or(self.key);
        if let Some(ref prefix) = self.prefix {
            write!(f, "[{prefix}] {title}")
        } else {
            write!(f, "{title}")
        }
    }
}

#[derive(Debug, Clone)]
struct TickerItem(Arc<str>);

#[derive(Debug)]
enum ProgressTick {
    TickerCreate(TickerData),
    TickerUpdate { key: &'static str, msg: Arc<str> },
    ItemOpen { key: &'static str, item: Arc<str> },
    ItemDone { key: &'static str, item: Arc<str> },
    TickerFinish { key: &'static str },
}

static TICKER: LazyLock<MultiProgressTicker> =
    LazyLock::new(|| MultiProgressWriter::new(!is_logging()).pipe(MultiProgressTicker::new));

#[inline]
pub fn stderr() -> impl Write {
    TICKER.writer()
}

/// Configure styling for a displayable value.
///
/// ## Note
///
/// Avoid using this in tracing messages.
///
/// tracing-subscriber currently escapes all ANSI characters.
/// See <https://github.com/tokio-rs/tracing/issues/3378>
///
/// This function disables styling if the application is in logging mode, so that
/// messages can have styling in tickers but not in logs.
#[inline]
pub fn styled<D>(val: D) -> StyledObject<D> {
    let styled = TICKER.style().apply_to(val);
    if TICKER.is_enabled() {
        styled
    } else {
        styled.force_styling(false)
    }
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
