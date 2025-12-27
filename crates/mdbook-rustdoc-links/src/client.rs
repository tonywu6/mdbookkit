use std::{
    collections::HashMap,
    ops::ControlFlow,
    process::Stdio,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use async_lsp::{LanguageServer, MainLoop, ServerSocket, router::Router};
use futures_util::TryFutureExt;
use lsp_types::{
    ClientCapabilities, ClientInfo, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GeneralClientCapabilities, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializeResult, InitializedParams, LogMessageParams, MessageType, NumberOrString, Position,
    PositionEncodingKind, ProgressParams, ProgressParamsValue, ServerInfo, ShowMessageParams,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WindowClientCapabilities, WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressEnd,
    WorkDoneProgressReport, WorkspaceFolder,
    notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage},
    request::Request,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{OnceCell, OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use tracing::{Instrument, Level, debug, debug_span, instrument, trace, warn, warn_span};

use mdbookkit::{
    emit_debug,
    env::is_logging,
    error::{ExpectLock, FutureWithError},
    logging::styled,
    ticker, ticker_event,
    url::UrlToPath,
};

use crate::{
    env::Environment,
    link::ItemLinks,
    sync::{Debounce, DebounceUpdate, Debouncing},
};

/// LSP client to talk to rust-analyzer.
///
/// [`Client`] does not implement [`Clone`], because the server instance is lazily spawned
/// and so must be unique for each client. To enable cloning, put this in an [`Arc`].
#[derive(Debug)]
pub struct Client {
    env: Environment,
    server: OnceCell<Server>,
    docs: DocumentLock,
}

impl Client {
    /// Initialize a new LSP client.
    ///
    /// This does not actually spawn rust-analyzer.
    pub fn new(env: Environment) -> Self {
        let server = OnceCell::new();
        let docs = DocumentLock::default();
        Self { env, server, docs }
    }

    pub fn env(&self) -> &Environment {
        &self.env
    }

    #[instrument("open_document", level = "debug", skip_all)]
    pub async fn open(&self, uri: Url, text: String) -> Result<OpenDocument> {
        trace!(%uri);

        let server = (self.server)
            .get_or_try_init(|| Server::spawn(&self.env))
            .await?
            .clone();

        let opened = self.docs.open(server.server.clone(), uri, text).await?;

        (server.debounce.wait())
            .await
            .context("Timed out waiting for rust-analyzer to finish indexing")?;

        Ok(opened)
    }

    /// Shutdown the server if it has been spawned.
    ///
    /// Returns the [`Environment`] struct for further use.
    pub async fn stop(mut self) -> Environment {
        if let Some(server) = self.server.take() {
            server
                .dispose()
                .context("Failed to properly shutdown LSP server")
                .inspect_err(emit_debug!())
                .await
                .ok();
        }
        self.env.clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // shutdown the server in a new task in case main thread bails
        // this avoids async-lsp's background thread panicking due to its
        // sender channel closing, resulting in a "Sender is alive" assertion
        if let Some(server) = self.server.take() {
            tokio::spawn(server.dispose());
        }
    }
}

#[derive(Debug, Clone)]
struct Server {
    server: ServerSocket,
    debounce: Debounce<()>,
    background: Arc<JoinHandle<()>>,
}

impl Server {
    #[instrument("spawn_lsp", level = "debug", skip_all)]
    async fn spawn(env: &Environment) -> Result<Self> {
        struct State {
            sender: mpsc::Sender<DebounceUpdate<()>>,
            ticker: Option<tracing::Span>,
            // this span is never entered, ticker/timing is updated on span close
            percent_indexed: Option<u32>,
            last_update: Option<String>,
        }

        impl State {
            fn ticker(&self) -> Option<tracing::span::Id> {
                self.ticker.as_ref()?.id()
            }
        }

        /// Listen for [Work Done Progress][workDoneProgress] events from rust-analyzer
        /// to determine if indexing is done.
        ///
        /// The `cachePriming` events look like this:
        ///
        /// - [`WorkDoneProgress::Begin`]
        /// - [`WorkDoneProgress::Report`] for each crate indexed, with messages like `4/200 (core)`
        /// - ...
        /// - [`WorkDoneProgress::End`]
        ///
        /// (This is the ticker thing that shows up in the VS Code status bar).
        ///
        /// Notably, rust-analyzer seems to do several rounds of indexing, **not all of
        /// which is actually indexing the entire codebase:**
        ///
        /// - First, a `0/1 (<crate name>)` round that only indexes the current crate.
        ///
        ///   - RA is **not ready** at this point: if we were to query for external docs
        ///     immediately after this [`WorkDoneProgress::End`] event, almost all links
        ///     will fail to resolve.
        ///
        /// - Then, a `0/x ...` round that seems to actually indexes everything, including
        ///   [`std`] and all the dependencies.
        ///
        ///   - RA is likely ready at this point.
        ///
        /// - Then, one or more additional rounds of indexing that finish very quickly.
        ///
        /// To be able to "reliably" determine that RA is ready for querying, the
        /// probing mechanism does essentially the following:
        ///
        /// 1. Ignore the first round of indexing.
        /// 2. Be extra pessimistic and wait for events to quiet down after receiving
        ///    the first [`WorkDoneProgress::End`] event; see [`EventSampler`].
        ///
        /// [workDoneProgress]: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#workDoneProgress
        #[instrument(level = "trace", skip(state))]
        fn probe_progress(state: &mut State, progress: ProgressParams) {
            match indexing_progress(&progress) {
                Some(IndexingProgress::CachePriming(WorkDoneProgress::Begin(begin))) => {
                    state.percent_indexed = Some(0);

                    let msg = begin.message.as_deref().unwrap_or_default();
                    ticker_event!(state.ticker(), Level::INFO, "{msg}");

                    let tx = state.sender.clone();
                    tokio::spawn(async move { tx.send(DebounceUpdate::Reset).await.ok() });
                }

                Some(IndexingProgress::CachePriming(WorkDoneProgress::Report(report))) => {
                    if let Some(msg) = report.message.as_deref()
                        && (state.last_update.as_deref())
                            .map(|last| last != msg)
                            .unwrap_or(true)
                    {
                        state.last_update = Some(msg.into());
                        ticker_event!(state.ticker(), Level::INFO, "{msg}");
                    }

                    let Some(indexed) = state.percent_indexed.as_mut() else {
                        trace!("progress was considered spurious");
                        return;
                    };

                    let update = report.percentage.unwrap_or(0);

                    let spurious = if let Some(msg) = &report.message {
                        // HACK: invalidate progress reports that say "0/1 (crate name)"
                        // because RA isn't actually indexing everything at this point
                        msg.starts_with("0/1 (")
                    } else {
                        // also ignore indexing runs that went from 0 to 100
                        *indexed == 0 && update == 100
                    };

                    trace!(update, spurious, "WorkDoneProgress::Report");

                    if spurious {
                        debug!("ignoring spurious rust-analyzer progress");
                        state.percent_indexed = None;
                        return;
                    }

                    if update >= *indexed {
                        *indexed = update;
                    }
                }

                Some(IndexingProgress::CachePriming(WorkDoneProgress::End(_))) => {
                    let Some(indexed) = state.percent_indexed else {
                        trace!("progress was considered spurious");
                        return;
                    };

                    trace!(indexed, "WorkDoneProgress::End");

                    if indexed < 99 {
                        return;
                    }

                    state.ticker.take();

                    let tx = state.sender.clone();
                    tokio::spawn(async move { tx.send(DebounceUpdate::Ready(())).await.ok() });
                }

                Some(IndexingProgress::Other(message)) => {
                    if let Some(ticker) = state.ticker() {
                        if is_logging() {
                            ticker_event!(ticker, Level::DEBUG, "{message}");
                        } else {
                            ticker_event!(ticker, Level::INFO, "{}", styled(message).dim());
                        }
                        let tx = state.sender.clone();
                        tokio::spawn(async move { tx.send(DebounceUpdate::Alive).await.ok() });
                    }
                }

                None => {}
            }
        }

        let (sender, receiver) = mpsc::channel(16);

        let debounce = Debouncing {
            debounce: Duration::from_millis(300),
            timeout: env.config.rust_analyzer_timeout(),
            receiver,
        }
        .build();

        let (background, mut server) = MainLoop::new_client(move |_| {
            let state = State {
                sender,
                ticker: Some(ticker!(
                    Level::INFO,
                    "rust-analyzer",
                    "rust-analyzer indexing"
                )),
                percent_indexed: Some(0),
                last_update: None,
            };

            let mut router = Router::new(state);

            router
                .notification::<Progress>(|state, progress| {
                    probe_progress(state, progress);
                    ControlFlow::Continue(())
                })
                .notification::<PublishDiagnostics>(|_, diagnostics| {
                    trace!("{diagnostics:?}");
                    ControlFlow::Continue(())
                })
                .notification::<ShowMessage>(|_, ShowMessageParams { typ, message }| {
                    log_message(message, typ);
                    ControlFlow::Continue(())
                })
                .notification::<LogMessage>(|_, LogMessageParams { typ, message }| {
                    log_message(message, typ);
                    ControlFlow::Continue(())
                })
                .event::<StopEvent>(|_, _| ControlFlow::Break(Ok(())));

            ServiceBuilder::new().service(router)
        });

        let mut proc = env
            .which()
            .command()?
            .current_dir(env.crate_dir.expect_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn rust-analyzer")?;

        let background = {
            let stdin = proc.stdin.take().expect("should have stdio");
            let stdout = proc.stdout.take().expect("should have stdio");
            let stderr = proc.stderr.take().expect("should have stdio");

            let (tx, rx) = oneshot::channel();

            tokio::spawn(
                async move {
                    let mut stderr = BufReader::new(stderr).lines();
                    let mut buffer = vec![];
                    while let Some(line) = stderr.next_line().await.ok().flatten() {
                        debug!("{line}");
                        buffer.push(line);
                    }
                    tx.send(buffer).ok();
                }
                .instrument(debug_span!("lsp_stderr")),
            );

            let background = tokio::spawn(
                async move {
                    if let Err(e) = background
                        .run_buffered(stdout.compat(), stdin.compat_write())
                        .await
                    {
                        warn!("LSP client stopped unexpectedly: {e}");
                        if let Some(stderr) = tokio::time::timeout(Duration::from_millis(100), rx)
                            .await
                            .into_iter()
                            .flatten()
                            .next()
                        {
                            warn!("Server process stderr:");
                            for line in stderr {
                                warn!("  {line}");
                            }
                        }
                    }
                }
                .instrument(warn_span!("lsp_thread")),
            );

            Arc::new(background)
        };

        let features = {
            let features = &env.config.cargo_features;
            if features.len() == 1 && features[0] == "all" {
                json!("all")
            } else {
                json!(features)
            }
        };

        let InitializeResult {
            server_info: info,
            capabilities,
        } = server
            .initialize(InitializeParams {
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: env.crate_dir.clone(),
                    name: "root".into(),
                }]),

                capabilities: ClientCapabilities {
                    experimental: Some(json! {{
                        "localDocs": true,
                    }}),
                    window: Some(WindowClientCapabilities {
                        work_done_progress: Some(true),
                        ..Default::default()
                    }),
                    general: Some(GeneralClientCapabilities {
                        position_encodings: Some(vec![PositionEncodingKind::UTF8]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },

                initialization_options: Some(json! {{
                    "cachePriming": {
                        "enable": true,
                        "numThreads": "physical",
                    },
                    "cargo": {
                        "features": features,
                    }
                }}),

                client_info: Some(ClientInfo {
                    name: env!("CARGO_PKG_NAME").into(),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                }),

                ..Default::default()
            })
            .context("Failed to initialize rust-analyzer")
            .await?;

        debug!("using rust-analyzer {info:?}");

        if capabilities.position_encoding != Some(PositionEncodingKind::UTF8) {
            let error = anyhow!("Found rust-analyzer version {}", ra_version(&info))
                .context("Server does not support utf-8 positions")
                .context("Unsupported rust-analyzer version");
            bail!(error)
        }

        server.initialized(InitializedParams {}).ok();

        Ok(Self {
            server,
            debounce,
            background,
        })
    }

    async fn dispose(self) -> Result<()> {
        let Self {
            mut server,
            background,
            ..
        } = self;
        server.shutdown(()).await?;
        server.exit(())?;
        server.emit(StopEvent)?;
        if let Some(background) = Arc::into_inner(background) {
            background.await?
        }
        Ok(())
    }
}

struct StopEvent;

#[derive(Debug, Default)]
struct DocumentLock {
    opened: RwLock<DocumentMap>,
}

type DocumentMap = HashMap<Url, (Arc<Semaphore>, i32)>;

impl DocumentLock {
    pub async fn open(
        &self,
        mut server: ServerSocket,
        uri: Url,
        text: String,
    ) -> Result<OpenDocument> {
        let (sema, version) = {
            let mut lock = self.opened.write().expect_lock();
            let (sema, version) = lock
                .entry(uri.clone())
                .or_insert_with(|| (Arc::new(Semaphore::new(1)), 0));
            *version += 1;
            (sema.clone(), *version)
        };

        let _permit = sema.acquire_owned().await?;

        server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                language_id: "rust".into(),
                uri: uri.clone(),
                text,
                version,
            },
        })?;

        Ok(OpenDocument {
            uri,
            server,
            _permit,
        })
    }
}

/// [OpenDocument] does not implement [Clone] because it will cause extraneous
/// `textDocument/didClose` notifications. Cloning should be done using [Arc].
#[derive(Debug)]
#[must_use = "bind this to a variable or document will be immediately closed"]
pub struct OpenDocument {
    uri: Url,
    server: ServerSocket,
    _permit: OwnedSemaphorePermit,
}

impl OpenDocument {
    #[instrument(level = "debug", skip(self))]
    pub async fn resolve(&self, position: Position) -> Result<ItemLinks> {
        let defs = self
            .server
            .clone()
            .definition(GotoDefinitionParams {
                text_document_position_params: document_position(self.uri.clone(), position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .context("Failed to request source definition")
            .inspect_err(emit_debug!())
            .await
            .unwrap_or_default()
            .map(|defs| match defs {
                GotoDefinitionResponse::Scalar(loc) => vec![loc.uri],
                GotoDefinitionResponse::Array(locs) => {
                    locs.into_iter().map(|loc| loc.uri).collect()
                }
                GotoDefinitionResponse::Link(links) => {
                    links.into_iter().map(|link| link.target_uri).collect()
                }
            })
            .unwrap_or_default();

        let ExternalDocLinks { web, local } = self
            .server
            .request::<ExternalDocs>(document_position(self.uri.clone(), position))
            .context("Failed to request external docs")
            .inspect_err(emit_debug!())
            .await
            .unwrap_or_default()
            .context("Server returned no result for external docs")?;

        ItemLinks::new(web, local, defs)
    }
}

impl Drop for OpenDocument {
    fn drop(&mut self) {
        self.server
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: self.uri.clone(),
                },
            })
            .context("Error sending textDocument/didClose")
            .inspect_err(emit_debug!())
            .ok();
    }
}

enum ExternalDocs {}

impl Request for ExternalDocs {
    const METHOD: &'static str = "experimental/externalDocs";
    type Params = TextDocumentPositionParams;
    type Result = Option<ExternalDocLinks>;
}

#[derive(Debug, Deserialize, Serialize)]
struct ExternalDocLinks {
    web: Option<Url>,
    local: Option<Url>,
}

fn document_position(uri: Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position,
    }
}

enum IndexingProgress<'a> {
    CachePriming(&'a WorkDoneProgress),
    Other(&'a String),
}

fn indexing_progress(progress: &ProgressParams) -> Option<IndexingProgress<'_>> {
    match progress {
        ProgressParams {
            token: NumberOrString::String(token),
            value: ProgressParamsValue::WorkDone(progress),
        } => {
            if matches!(
                token.as_ref(),
                "rustAnalyzer/Indexing" | "rustAnalyzer/cachePriming"
            ) {
                Some(IndexingProgress::CachePriming(progress))
            } else if let WorkDoneProgress::Begin(WorkDoneProgressBegin {
                message: Some(message),
                ..
            })
            | WorkDoneProgress::Report(WorkDoneProgressReport {
                message: Some(message),
                ..
            })
            | WorkDoneProgress::End(WorkDoneProgressEnd {
                message: Some(message),
                ..
            }) = progress
            {
                Some(IndexingProgress::Other(message))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn log_message(message: String, typ: MessageType) {
    match typ {
        MessageType::ERROR | MessageType::WARNING | MessageType::INFO | MessageType::LOG => {
            debug!("rust-analyzer: {message}")
        }
        _ => trace!("rust-analyzer: {message}"),
    }
}

fn ra_version(info: &Option<ServerInfo>) -> &str {
    info.as_ref()
        .and_then(|ServerInfo { version, .. }| version.as_deref())
        .unwrap_or("(unknown)")
}
