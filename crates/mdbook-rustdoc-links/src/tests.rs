#![allow(clippy::unwrap_used)]

use anyhow::{Context, Result, bail};
use lsp_types::Url;
use rstest::*;
use similar::{ChangeTag, TextDiff};
use tap::Pipe;
use tracing::level_filters::LevelFilter;

use mdbookkit::{logging::Logging, portable_snapshots, test_document, testing::TestDocument};

use crate::{
    client::Client,
    env::{Config, Environment},
    link_report,
    page::Pages,
    resolver::Resolver,
};

struct Fixture {
    pages: Pages<'static, Url>,
    env: Environment,
}

#[fixture]
#[once]
fn fixture() -> Fixture {
    Logging {
        logging: Some(true),
        colored: Some(false),
        level: LevelFilter::DEBUG,
    }
    .init();

    let client = Config {
        rust_analyzer: Some("cargo run --package util-rust-analyzer -- analyzer".into()),
        ..Default::default()
    }
    .pipe(Environment::new)
    .context("Failed to initialize environment")
    .unwrap()
    .pipe(Client::new);

    let mut pages = Pages::default();

    for doc in TEST_DOCUMENTS {
        let stream = client.env().markdown(doc.content).into_offset_iter();
        pages
            .read(doc.url(), doc.content, stream)
            .context("Failed to parse source")
            .unwrap();
    }

    let env = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            client
                .resolve(&mut pages)
                .await
                .context("Failed to resolve links")
                .unwrap();
            client.stop().await
        });

    link_report(&pages);

    Fixture { env, pages }
}

fn assert_output(doc: TestDocument, Fixture { pages, env }: &Fixture) -> Result<()> {
    let output = pages.emit(&doc.url(), &env.emit_config())?;
    portable_snapshots!().test(doc.name(), |name| insta::assert_snapshot!(name, output))?;
    Ok(())
}

fn assert_report(doc: TestDocument, Fixture { pages, .. }: &Fixture) -> Result<()> {
    let report = pages
        .reporter()
        .name_display(|_| doc.name())
        .level_filter(LevelFilter::DEBUG)
        .filtered(|u| u == &doc.url())
        .build()
        .to_report();

    portable_snapshots!().test(format!("{}.stderr", doc.name()), |name| {
        insta::assert_snapshot!(name, report)
    })?;

    Ok(())
}

fn assert_whitespace_unchanged(doc: TestDocument, Fixture { pages, env }: &Fixture) -> Result<()> {
    let output = pages.emit(&doc.url(), &env.emit_config())?;

    let changed_lines = TextDiff::from_words(doc.content, &output)
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
        bail!("Unexpected whitespace change: {changed_lines:?}")
    } else {
        Ok(())
    }
}

macro_rules! test_documents {
    ( $($path:literal,)+ ) => {
        static TEST_DOCUMENTS: &[TestDocument] = &[
            $(test_document!($path),)*
        ];

        #[rstest]
        $(#[case(test_document!($path))])*
        fn assert_outputs(#[case] doc: TestDocument, fixture: &Fixture) -> Result<()> {
            assert_output(doc, fixture)
        }

        #[rstest]
        $(#[case(test_document!($path))])*
        fn assert_reports(#[case] doc: TestDocument, fixture: &Fixture) -> Result<()> {
            assert_report(doc, fixture)
        }

        #[rstest]
        $(#[case(test_document!($path))])*
        fn check_whitespace(#[case] doc: TestDocument, fixture: &Fixture) -> Result<()> {
            assert_whitespace_unchanged(doc, fixture)
        }
    };
}

test_documents![
    "../../../docs/src/rustdoc-links/index.md",
    "../../../docs/src/rustdoc-links/getting-started.md",
    "../../../docs/src/rustdoc-links/supported-syntax.md",
    "../../../docs/src/rustdoc-links/known-issues.md",
    "tests/ra-known-quirks.md",
];
