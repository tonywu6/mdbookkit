use std::sync::Arc;

use anyhow::Result;

use mdbook_rustdoc_link::{env::Environment, logger::ConsoleLogger, Client, ClientConfig};
use tap::Pipe;
use tokio::task::JoinSet;
use util_testing::{portable_snapshots, test_document, TestDocument};

async fn snapshot(client: Arc<Client>, TestDocument { source, name }: TestDocument) -> Result<()> {
    let output = client.process(source).await?;
    portable_snapshots!().test(|| insta::assert_snapshot!(name, output))
}

#[tokio::test]
async fn test_snapshots() -> Result<()> {
    let client = client()?;

    let tests = [
        test_document!("../../../docs/src/rustdoc-link/supported-syntax.md"),
        test_document!("../../../docs/src/rustdoc-link.md"),
        test_document!("tests/ra-known-quirks.md"),
    ];

    let errors = tests
        .map(|test| snapshot(client.clone(), test))
        .into_iter()
        .collect::<JoinSet<_>>()
        .join_all()
        .await
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();

    if !errors.is_empty() {
        let errors = errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("{errors}")
    }

    client.dispose_shared().await?;

    Ok(())
}

fn client() -> Result<Arc<Client>> {
    ConsoleLogger::init();
    ClientConfig {
        rust_analyzer: Some("cargo run --release --package util-rust-analyzer --".into()),
        ..Default::default()
    }
    .pipe(Environment::new)?
    .pipe(Client::new)
    .pipe(Arc::new)
    .pipe(Ok)
}
