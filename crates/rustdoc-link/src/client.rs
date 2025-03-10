use std::{
    collections::HashMap,
    ops::ControlFlow,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use async_lsp::{
    concurrency::ConcurrencyLayer, panic::CatchUnwindLayer, router::Router, tracing::TracingLayer,
    LanguageServer, MainLoop, ServerSocket,
};
use cargo_toml::{Manifest, Product};
use lsp_types::{
    notification::{Progress, PublishDiagnostics, ShowMessage},
    request::Request,
    ClientCapabilities, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GeneralClientCapabilities, InitializeParams, InitializedParams, NumberOrString, Position,
    PositionEncodingKind, ProgressParams, ProgressParamsValue, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Url, WindowClientCapabilities, WorkDoneProgress,
    WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    process::Command,
    sync::{mpsc, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::timeout,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

use crate::{sync::Subsample, BuildOptions};

#[derive(Debug, Clone)]
pub struct Client {
    pub server: ServerSocket,
    pub config: Environment,
    stabilizer: Subsample<()>,
    documents: DocumentLock,
}

impl Client {
    pub async fn spawn(options: BuildOptions) -> Result<(Self, DisposeClient)> {
        let config = Environment::new(options)?;

        let (tx, rx) = mpsc::channel(16);

        let stabilizer = Subsample::new(rx, Duration::from_secs(1));

        let (background, mut server) = MainLoop::new_client(move |_| {
            struct State {
                tx: mpsc::Sender<Poll<()>>,
            }

            let state = State { tx };

            let mut router = Router::new(state);

            router
                .notification::<Progress>(|state, progress| {
                    log::debug!("{progress:#?}");

                    match indexing_progress(&progress) {
                        None => log::debug!("{progress:#?}"),
                        Some(WorkDoneProgress::Begin(begin)) => {
                            log::info!(
                                "{} {}",
                                begin.title,
                                begin.message.as_deref().unwrap_or_default()
                            );
                            let tx = state.tx.clone();
                            tokio::spawn(async move { tx.send(Poll::Pending).await.ok() });
                        }
                        Some(WorkDoneProgress::End(end)) => {
                            log::info!("{}", end.message.as_deref().unwrap_or("Done"));
                            let tx = state.tx.clone();
                            tokio::spawn(async move { tx.send(Poll::Ready(())).await.ok() });
                        }
                        Some(WorkDoneProgress::Report(report)) => {
                            if let Some(message) = &report.message {
                                log::info!("{message}");
                            }
                        }
                    }

                    ControlFlow::Continue(())
                })
                .notification::<PublishDiagnostics>(|_, diagnostics| {
                    log::debug!("{diagnostics:#?}");
                    ControlFlow::Continue(())
                })
                .notification::<ShowMessage>(|_, message| {
                    log::debug!("{message:#?}");
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
            .current_dir(&config.root_dir)
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
        });

        let root_uri = Url::from_directory_path(&config.root_dir).unwrap();

        let init = server
            .initialize(InitializeParams {
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: root_uri.clone(),
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

        log::debug!("{init:#?}");

        if init.capabilities.position_encoding != Some(PositionEncodingKind::UTF8) {
            bail!("this rust-analyzer does not support utf-8 positions")
        }

        server.initialized(InitializedParams {})?;

        let documents = DocumentLock::default();

        let client = Self {
            server,
            config,
            stabilizer,
            documents,
        };

        Ok((client, DisposeClient(background)))
    }

    pub async fn open(&self, uri: Url, text: String) -> Result<OpenDocument> {
        let document = self.documents.open(self.server.clone(), uri, text).await?;
        timeout(Duration::from_secs(60), self.stabilizer.wait())
            .await
            .context("timed out waiting for rust-analyzer to open document")?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub root_dir: PathBuf,
    pub entrypoint: Url,
    pub build_opts: BuildOptions,
}

impl Environment {
    fn new(build_opts: BuildOptions) -> Result<Self> {
        let root_dir = build_opts
            .manifest_dir
            .clone()
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)?
            .canonicalize()?;

        let entrypoint = find_entrypoint(&root_dir)?;

        Ok(Self {
            root_dir,
            entrypoint,
            build_opts,
        })
    }
}

pub fn find_entrypoint<P: AsRef<Path>>(from_dir: P) -> Result<Url> {
    let dir = from_dir.as_ref().canonicalize()?;

    let path = {
        let mut dir = dir.as_path();
        loop {
            let path = dir.join("Cargo.toml");
            if path.exists() {
                break path;
            }
            dir = match dir.parent() {
                Some(dir) => dir,
                None => {
                    return Err(anyhow!(from_dir.as_ref().display().to_string()))
                        .context("failed to find a Cargo.toml");
                }
            };
        }
    };

    let manifest = {
        let mut manifest = Manifest::from_path(&path)?;
        manifest.complete_from_path(&path)?;
        manifest
    };

    let root_url = Url::from_file_path(&path).unwrap();

    if let Some(Product {
        path: Some(lib), ..
    }) = manifest.lib
    {
        Ok(root_url.join(&lib)?)
    } else if let Some(bin) = manifest.bin.iter().find_map(|bin| bin.path.as_ref()) {
        Ok(root_url.join(bin)?)
    } else {
        Err(anyhow!("{}", path.display())).context("Cargo.toml does not have a lib or bin target")
    }
}

struct StopEvent;

#[must_use]
pub struct DisposeClient(JoinHandle<()>);

impl DisposeClient {
    pub async fn of(self, client: Client) -> Result<()> {
        let Client { mut server, .. } = client;
        server.shutdown(()).await?;
        server.exit(())?;
        server.emit(StopEvent)?;
        self.0.await?;
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
struct DocumentLock {
    opened: Arc<RwLock<DocumentMap>>,
}

type DocumentMap = HashMap<Url, (Arc<Semaphore>, i32)>;

impl DocumentLock {
    pub async fn open(
        &self,
        mut sock: ServerSocket,
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

        sock.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                language_id: "rust".into(),
                uri: uri.clone(),
                text,
                version,
            },
        })?;

        let closing = Some(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        });

        Ok(OpenDocument {
            sock,
            closing,
            _permit,
        })
    }
}

#[derive(Debug)]
#[must_use = "bind this to a variable or document will be immediately closed"]
pub struct OpenDocument {
    sock: ServerSocket,
    closing: Option<DidCloseTextDocumentParams>,
    _permit: OwnedSemaphorePermit,
}

impl Drop for OpenDocument {
    fn drop(&mut self) {
        self.sock.did_close(self.closing.take().unwrap()).ok();
    }
}

pub enum ExternalDocs {}

impl Request for ExternalDocs {
    const METHOD: &'static str = "experimental/externalDocs";
    type Params = TextDocumentPositionParams;
    type Result = Option<ExternalDocLinks>;
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExternalDocLinks {
    pub web: Option<Url>,
    pub local: Option<Url>,
}

pub fn document_position(uri: Url, position: Position) -> TextDocumentPositionParams {
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
