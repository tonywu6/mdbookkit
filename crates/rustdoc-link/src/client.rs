use std::{
    collections::HashMap,
    ops::ControlFlow,
    process::Stdio,
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_lsp::{
    concurrency::ConcurrencyLayer, panic::CatchUnwindLayer, router::Router, tracing::TracingLayer,
    LanguageServer, MainLoop, ServerSocket,
};
use lsp_types::{
    notification::{Progress, PublishDiagnostics, ShowMessage},
    request::Request,
    ClientCapabilities, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GeneralClientCapabilities, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializedParams, NumberOrString, Position, PositionEncodingKind, ProgressParams,
    ProgressParamsValue, TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WindowClientCapabilities, WorkDoneProgress, WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tap::{Pipe, TapFallible};
use tokio::{
    process::Command,
    sync::{mpsc, OnceCell, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

use crate::{env::Environment, log_debug, log_warning, sync::Subsample, terminal::spinner};

#[derive(Debug, Clone)]
pub struct Client {
    pub env: Environment,
    server: OnceCell<Server>,
    docs: DocumentLock,
}

impl Client {
    pub fn new(env: Environment) -> Self {
        let server = OnceCell::new();
        let docs = DocumentLock::default();
        Self { env, server, docs }
    }

    pub async fn open(&self, uri: Url, text: String) -> Result<OpenDocument> {
        let server = self
            .server
            .get_or_try_init(|| Server::spawn(&self.env))
            .await?
            .clone();

        let document = self.docs.open(server.server, uri, text).await?;

        timeout(Duration::from_secs(60), server.stabilizer.wait())
            .await
            .context("timed out waiting for rust-analyzer to finish indexing")?;

        Ok(document)
    }

    pub async fn dispose(self) -> Result<()> {
        if let Some(server) = self.server.into_inner() {
            server.dispose().await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Server {
    server: ServerSocket,
    stabilizer: Subsample<()>,
    background: Arc<JoinHandle<()>>,
}

impl Server {
    async fn spawn(env: &Environment) -> Result<Self> {
        macro_rules! ra_spinner {
            () => {
                "rust-analyzer"
            };
        }

        let (tx, rx) = mpsc::channel(16);

        let stabilizer = Subsample::new(rx, Duration::from_secs(1));

        let (background, mut server) = MainLoop::new_client(move |_| {
            struct State {
                tx: mpsc::Sender<Poll<()>>,
                percent_indexed: u32,
            }

            let state = State {
                tx,
                percent_indexed: 0,
            };

            let mut router = Router::new(state);

            router
                .notification::<Progress>(|state, progress| {
                    match indexing_progress(&progress) {
                        Some(WorkDoneProgress::Begin(begin)) => {
                            state.percent_indexed = 0;

                            let msg = begin.message.as_deref().unwrap_or_default();
                            spinner().update(ra_spinner!(), msg);

                            let tx = state.tx.clone();
                            tokio::spawn(async move { tx.send(Poll::Pending).await.ok() });
                        }

                        Some(WorkDoneProgress::End(end)) => {
                            if state.percent_indexed >= 99 {
                                let msg = end.message.as_deref().unwrap_or("indexing done");
                                spinner().update(ra_spinner!(), msg);

                                let tx = state.tx.clone();
                                tokio::spawn(async move { tx.send(Poll::Ready(())).await.ok() });
                            }
                        }

                        Some(WorkDoneProgress::Report(report)) => {
                            if let Some(pc) = report.percentage {
                                if pc >= state.percent_indexed {
                                    state.percent_indexed = pc;
                                }
                            }
                            if let Some(message) = &report.message {
                                spinner().update(ra_spinner!(), message);
                            }
                        }

                        None => {}
                    }

                    ControlFlow::Continue(())
                })
                .notification::<PublishDiagnostics>(|_, diagnostics| {
                    log::trace!("{diagnostics:#?}");
                    ControlFlow::Continue(())
                })
                .notification::<ShowMessage>(|_, message| {
                    log::trace!("{message:#?}");
                    ControlFlow::Continue(())
                })
                .event(|_, _: StopEvent| ControlFlow::Break(Ok(())));

            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let proc = Command::new("rust-analyzer")
            .current_dir(env.crate_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("failed to spawn rust-analyzer")?;

        let background = tokio::spawn(async move {
            let mut proc = proc;
            let stdout = proc.stdout.take().unwrap();
            let stdin = proc.stdin.take().unwrap();
            background
                .run_buffered(stdout.compat(), stdin.compat_write())
                .await
                .unwrap();
        })
        .pipe(Arc::new);

        let init = server
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
                ..Default::default()
            })
            .await?;

        log::trace!("{init:#?}");

        if init.capabilities.position_encoding != Some(PositionEncodingKind::UTF8) {
            bail!("this rust-analyzer does not support utf-8 positions")
        }

        server.initialized(InitializedParams {})?;

        spinner().create(ra_spinner!(), None);

        Ok(Self {
            server,
            stabilizer,
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
        Arc::into_inner(background)
            .expect("dispose called while multiple references exist")
            .await?;
        Ok(())
    }
}

struct StopEvent;

#[derive(Debug, Default, Clone)]
struct DocumentLock {
    opened: Arc<RwLock<DocumentMap>>,
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

        Ok(OpenDocument {
            uri,
            server,
            _permit,
        })
    }
}

/// [OpenDocument] does not implement [Clone] because it will cause extraneous
/// `textDocument/didClose` notifications. Cloning should be done via [Arc].
#[derive(Debug)]
#[must_use = "bind this to a variable or document will be immediately closed"]
pub struct OpenDocument {
    uri: Url,
    server: ServerSocket,
    _permit: OwnedSemaphorePermit,
}

impl OpenDocument {
    pub async fn resolve(&self, position: Position) -> Result<ItemLinks> {
        let sources = self
            .server
            .clone()
            .definition(GotoDefinitionParams {
                text_document_position_params: document_position(self.uri.clone(), position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .context("failed to request for source definition")
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
            .context("failed to request for external docs")
            .tap_err(log_warning!())
            .unwrap_or_default()
            .context("server returned no result for external docs")?;

        if web.is_none() && local.is_none() {
            bail!("server returned no result for external docs");
        } else {
            Ok(ItemLinks {
                web,
                local,
                defs: sources,
            })
        }
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
            .context("error sending textDocument/didClose")
            .tap_err(log_debug!())
            .ok();
    }
}

#[derive(Debug, Default)]
pub struct ItemLinks {
    pub web: Option<Url>,
    pub local: Option<Url>,
    pub defs: Vec<Url>,
}

impl ItemLinks {
    pub fn is_empty(&self) -> bool {
        self.web.is_none() && self.local.is_none()
    }

    pub fn with_fragment(mut self, fragment: Option<&str>) -> Self {
        if let Some(fragment) = fragment {
            if let Some(web) = &mut self.web {
                web.set_fragment(Some(fragment));
            }
            if let Some(local) = &mut self.local {
                local.set_fragment(Some(fragment));
            }
        }
        self
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
    if let ProgressParams {
        token: NumberOrString::String(token),
        value: ProgressParamsValue::WorkDone(progress),
    } = progress
    {
        if token == "rustAnalyzer/Indexing" {
            Some(progress)
        } else {
            None
        }
    } else {
        None
    }
}
