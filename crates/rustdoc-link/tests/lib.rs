use anyhow::Result;
use crate_testing::{portable_snapshots, test_document};

use mdbook_rustdoc_link::{env::Environment, Client, ClientConfig};

#[tokio::test]
async fn test_snapshots() -> Result<()> {
    let config = ClientConfig {
        rust_analyzer: Some("cargo run --release --package rust-analyzer --".into()),
        ..Default::default()
    };

    let client = Client::new(Environment::new(config)?);

    let tests = [
        test_document!("../../../docs/src/rustdoc-link/supported-syntax.md"),
        test_document!("tests/ra-known-quirks.md"),
    ];

    let mut errors = vec![];

    for test in tests {
        let output = match client.process(test.source).await {
            Ok(output) => output,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        portable_snapshots!().test(|| insta::assert_snapshot!(test.name, output))?;
    }

    client.dispose().await?;

    if !errors.is_empty() {
        let errors = errors
            .into_iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("{errors}")
    }

    Ok(())
}
