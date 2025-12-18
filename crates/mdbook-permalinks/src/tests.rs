use std::sync::{Arc, Mutex};

use anyhow::Result;
use git2::Repository;
use log::LevelFilter;
use rstest::*;
use url::Url;

use mdbookkit::{
    markdown::default_markdown_options,
    portable_snapshots, test_document,
    testing::{CARGO_WORKSPACE_DIR, TestDocument, setup_logging},
};

use crate::{Config, Environment, VersionControl, link::LinkStatus, page::Pages, vcs::Permalink};

struct Fixture {
    pages: Pages<'static>,
    env: Arc<Mutex<Environment>>,
}

#[fixture]
#[once]
fn fixture() -> Fixture {
    (|| -> Result<_> {
        setup_logging(env!("CARGO_PKG_NAME"));

        let env = Environment {
            vcs: VersionControl {
                root: CARGO_WORKSPACE_DIR.clone(),
                link: Permalink {
                    template: "https://example.org/git/{tree}/{ref}/{path}"
                        .parse()
                        .unwrap(),
                    reference: "v0.0".into(),
                },
                repo: Repository::open_from_env().unwrap(),
            },
            book_src: CARGO_WORKSPACE_DIR
                .join("crates/")?
                .join(concat!(env!("CARGO_PKG_NAME"), "/"))?
                .join("src/")?,
            markdown: default_markdown_options(),
            config: Config {
                book_url: Some("https://example.org/book".parse::<Url>()?.into()),
                ..Default::default()
            },
        };

        let mut pages = Pages::new(default_markdown_options());

        for doc in TEST_DOCUMENTS {
            pages.insert(doc.url(), doc.content)?;
        }

        env.resolve(&mut pages);

        let env = Arc::new(Mutex::new(env));

        Ok(Fixture { env, pages })
    })()
    .unwrap()
}

fn assert_output(doc: TestDocument, fixture: &Fixture) -> Result<()> {
    let output = fixture.pages.emit(&doc.url())?;
    portable_snapshots!().test(|| insta::assert_snapshot!(doc.name(), output))?;
    Ok(())
}

macro_rules! test_output {
    ( $($path:literal,)* ) => {
        static TEST_DOCUMENTS: &[TestDocument] = &[$(
            test_document!($path)
        ),*];

        #[rstest]
        $(#[case(test_document!($path))])*
        fn test_output(#[case] doc: TestDocument, fixture: &Fixture) -> Result<()> {
            assert_output(doc, fixture)
        }
    };
}

test_output!["tests/paths.md", "tests/urls.md", "tests/suffix.md",];

macro_rules! matcher {
    ( $pattern:pat ) => {
        |status: &LinkStatus| matches!(status, $pattern)
    };
}

#[rstest]
#[case("_stderr.ignored", matcher!(LinkStatus::Ignored))]
#[case("_stderr.published", matcher!(LinkStatus::Unchanged))]
#[case("_stderr.rewritten", matcher!(LinkStatus::Rewritten))]
#[case("_stderr.permalink", matcher!(LinkStatus::Permalink))]
#[case("_stderr.unreachable", matcher!(LinkStatus::Unreachable(..)))]
#[case("_stderr.link-error", matcher!(LinkStatus::Error(..)))]
fn test_stderr(
    #[case] name: &str,
    #[case] test: impl Fn(&LinkStatus) -> bool,
    fixture: &Fixture,
) -> Result<()> {
    let Fixture { env, pages } = fixture;
    let env = env.lock().unwrap();
    let report = env
        .report_issues(pages, test)
        .level(LevelFilter::Debug)
        .names(|url| env.rel_path(url))
        .colored(false)
        .logging(false)
        .build()
        .to_report();
    drop(env);
    portable_snapshots!().test(|| insta::assert_snapshot!(name, report))?;
    Ok(())
}
