use std::sync::Arc;

use anyhow::{bail, Result};

use mdbook_rustdoc_link::{
    env::{Config, Environment},
    logger::ConsoleLogger,
    Client,
};
use similar::{ChangeTag, TextDiff};
use tap::Pipe;
use tokio::task::JoinSet;
use util_testing::{portable_snapshots, test_document, TestDocument};

async fn snapshot(client: Arc<Client>, TestDocument { source, name }: TestDocument) -> Result<()> {
    let output = client.process(source).await?;

    portable_snapshots!().test(|| insta::assert_snapshot!(name, output))?;
    assert_no_whitespace_change(source, &output)?;

    Ok(())
}

fn assert_no_whitespace_change(source: &str, output: &str) -> Result<()> {
    let changed_lines = TextDiff::from_words(source, output)
        .iter_all_changes()
        .filter_map(|change| {
            if matches!(change.tag(), ChangeTag::Equal) {
                return None;
            }
            if change.value().contains('\n') {
                Some(change.value())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if !changed_lines.is_empty() {
        bail!("unexpected whitespace change: {changed_lines:?}")
    } else {
        Ok(())
    }
}

#[tokio::test]
async fn test_snapshots() -> Result<()> {
    let client = setup()?;

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

    client.drop().await?;

    Ok(())
}

fn setup() -> Result<Arc<Client>> {
    ConsoleLogger::init();
    Config {
        rust_analyzer: Some("cargo run --release --package util-rust-analyzer --".into()),
        ..Default::default()
    }
    .pipe(Environment::new)?
    .pipe(Client::new)
    .pipe(Arc::new)
    .pipe(Ok)
}
