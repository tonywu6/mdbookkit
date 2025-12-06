use std::{
    collections::HashMap,
    ops::ControlFlow,
    process::Stdio,
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use async_lsp::{LanguageServer, MainLoop, ServerSocket, router::Router};
use lsp_types::{
    ClientCapabilities, ClientInfo, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GeneralClientCapabilities, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializeResult, InitializedParams, LogMessageParams, MessageType, NumberOrString, Position,
    PositionEncodingKind, ProgressParams, ProgressParamsValue, ServerInfo, ShowMessageParams,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WindowClientCapabilities, WorkDoneProgress, WorkspaceFolder,
    notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage},
    request::Request,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tap::{Pipe, TapFallible};
use tokio::{
    sync::{OnceCell, OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

use mdbookkit::{log_debug, log_warning, logging::spinner};

use super::{
    env::Environment,
    link::ItemLinks,
    sync::{EventSampler, EventSampling},
    url::UrlToPath,
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

    pub async fn open(&self, uri: Url, text: String) -> Result<OpenDocument> {
        let server = self
            .server
            .get_or_try_init(|| Server::spawn(&self.env))
            .await?
            .clone();

        let opened = self.docs.open(server.server.clone(), uri, text).await?;

        server
            .stabilizer
            .wait()
            .await
            .with_context(|| format!("using rust-analyzer version {}", ra_version(&server.info)))
            .context("timed out waiting for rust-analyzer to finish indexing")?;

        Ok(opened)
    }

    /// Shutdown the server if it was spawned.
    ///
    /// Returns the [`Environment`] struct for further use.
    pub async fn stop(self) -> Environment {
        if let Some(server) = self.server.into_inner() {
            server
                .dispose()
                .await
                .context("failed to properly stop rust-analyzer")
                .tap_err(log_warning!())
                .ok();
        }
        self.env
    }
}

#[derive(Debug, Clone)]
struct Server {
    server: ServerSocket,
    stabilizer: EventSampler<()>,
    background: Arc<JoinHandle<()>>,
    info: Option<ServerInfo>,
}

impl Server {
    async fn spawn(env: &Environment) -> Result<Self> {
        macro_rules! ra_spinner {
            () => {
                "rust-analyzer"
            };
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
        fn probe_progress(state: &mut State, progress: ProgressParams) {
            match indexing_progress(&progress) {
                Some(WorkDoneProgress::Begin(begin)) => {
                    state.percent_indexed = Some(0);

                    let msg = begin.message.as_deref().unwrap_or_default();
                    spinner().update(ra_spinner!(), msg);

                    let tx = state.tx.clone();
                    tokio::spawn(async move { tx.send(Poll::Pending).await.ok() });
                }

                Some(WorkDoneProgress::Report(report)) => {
                    if let Some(msg) = &report.message {
                        spinner().update(ra_spinner!(), msg);
                    }

                    let Some(indexed) = state.percent_indexed.as_mut() else {
                        // progress was invalidated
                        return;
                    };

                    let update = report.percentage.unwrap_or(0);

                    let spurious = if let Some(msg) = &report.message {
                        // HACK: invalidate progress reports that say "0/1 (crate name)"
                        // because RA isn't actually indexing everything at this point
                        msg.starts_with("0/1 (")
                    } else {
                        // also ignore indexing runs that went from 0 to a 100
                        *indexed == 0 && update == 100
                    };

                    if spurious {
                        log::debug!("ignoring spurious rust-analyzer progress");
                        state.percent_indexed = None;
                        return;
                    }

                    if update >= *indexed {
                        *indexed = update;
                    }
                }

                Some(WorkDoneProgress::End(end)) => {
                    let Some(indexed) = state.percent_indexed else {
                        // progress was invalidated
                        return;
                    };

                    if indexed < 99 {
                        return;
                    }

                    let msg = end.message.as_deref().unwrap_or("indexing done");
                    spinner().update(ra_spinner!(), msg);

                    let tx = state.tx.clone();
                    tokio::spawn(async move { tx.send(Poll::Ready(())).await.ok() });
                }

                None => {
                    log::trace!("{progress:#?}")
                }
            }
        }

        let (tx, rx) = mpsc::channel(16);

        let stabilizer = EventSampling {
            buffer: Duration::from_millis(500),
            timeout: env.config.rust_analyzer_timeout(),
        }
        .using(rx);

        struct State {
            tx: mpsc::Sender<Poll<()>>,
            percent_indexed: Option<u32>,
        }

        let (background, mut server) = MainLoop::new_client(move |_| {
            let state = State {
                tx,
                percent_indexed: Some(0),
            };

            let mut router = Router::new(state);

            router
                .notification::<Progress>(|state, progress| {
                    log::trace!("{progress:#?}");
                    probe_progress(state, progress);
                    ControlFlow::Continue(())
                })
                .notification::<PublishDiagnostics>(|_, diagnostics| {
                    log::trace!("{diagnostics:#?}");
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

        let proc = env
            .which()
            .command()?
            .current_dir(env.crate_dir.to_path()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // TODO: managed subcommand stderr
            .stderr(Stdio::null())
            .spawn()
            .context("failed to spawn rust-analyzer")?;

        let background = tokio::spawn(async move {
            let mut proc = proc;
            let stdout = proc.stdout.take().unwrap();
            let stdin = proc.stdin.take().unwrap();
            background
                .run_buffered(stdout.compat(), stdin.compat_write())
                .await
                .tap_err(log_debug!())
                .ok();
        })
        .pipe(Arc::new);

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
            .await?;

        if capabilities.position_encoding != Some(PositionEncodingKind::UTF8) {
            let version = ra_version(&info);
            let error = anyhow!("using rust-analyzer version {version}")
                .context("this rust-analyzer does not support utf-8 positions");
            bail!(error)
        }

        server.initialized(InitializedParams {})?;

        spinner().create(ra_spinner!(), None);

        Ok(Self {
            server,
            stabilizer,
            background,
            info,
        })
    }

    async fn dispose(self) -> Result<()> {
        let Self {
            mut server,
            background,
            ..
        } = self;
        let background = Arc::into_inner(background)
            .expect("should not dispose while multiple server sockets are still alive");
        server.shutdown(()).await?;
        server.exit(())?;
        server.emit(StopEvent)?;
        background.await?;
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
            let mut lock = self.opened.write().unwrap();
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

        log::debug!("textDocument/didOpen {}", uri);

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
    pub async fn resolve(&self, position: Position) -> Result<ItemLinks> {
        let defs = self
            .server
            .clone()
            .definition(GotoDefinitionParams {
                text_document_position_params: document_position(self.uri.clone(), position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("failed to request source definition")
            .tap_err(log_warning!())
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
            .await
            .context("failed to request external docs")
            .tap_err(log_warning!())
            .unwrap_or_default()
            .context("server returned no result for external docs")?;

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
            .tap_ok(|_| log::debug!("textDocument/didClose {}", self.uri))
            .context("error sending textDocument/didClose")
            .tap_err(log_debug!())
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

fn indexing_progress(progress: &ProgressParams) -> Option<&WorkDoneProgress> {
    match progress {
        ProgressParams {
            token: NumberOrString::String(token),
            value: ProgressParamsValue::WorkDone(progress),
        } if matches!(
            token.as_ref(),
            "rustAnalyzer/Indexing" | "rustAnalyzer/cachePriming"
        ) =>
        {
            Some(progress)
        }
        _ => None,
    }
}

fn log_message(message: String, typ: MessageType) {
    match typ {
        MessageType::ERROR | MessageType::WARNING => {
            log::warn!("rust-analyzer: {message}")
        }
        MessageType::INFO | MessageType::LOG => {
            log::debug!("rust-analyzer: {message}")
        }
        _ => log::trace!("rust-analyzer: {message}"),
    }
}

fn ra_version(info: &Option<ServerInfo>) -> &str {
    info.as_ref()
        .and_then(|ServerInfo { version, .. }| version.as_deref())
        .unwrap_or("(unknown)")
}
