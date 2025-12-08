use std::sync::LazyLock;

use anyhow::Result;
use log::LevelFilter;
use rstest::*;
use tap::Pipe;
use url::Url;

use mdbookkit::{
    markdown::mdbook_markdown_options,
    portable_snapshots, test_document,
    testing::{CARGO_WORKSPACE_DIR, TestDocument, setup_logging},
};

use crate::{
    Config, Environment, VersionControl,
    link::LinkStatus,
    page::Pages,
    vcs::{GitHubPermalink, Permalink},
};

struct Fixture {
    env: Environment,
    pages: Pages<'static>,
}

static FIXTURE: LazyLock<Fixture> = LazyLock::new(|| {
    (|| -> Result<_> {
        setup_logging(env!("CARGO_PKG_NAME"));

        let env = Environment {
            vcs: VersionControl {
                root: CARGO_WORKSPACE_DIR.clone(),
                link: GitHubPermalink::new("lorem", "ipsum", "dolor").pipe(Permalink::GitHub),
            },
            book_src: CARGO_WORKSPACE_DIR
                .join("crates/")?
                .join(concat!(env!("CARGO_PKG_NAME"), "/"))?
                .join("src/")?,
            markdown: mdbook_markdown_options(),
            config: Config {
                book_url: Some("https://example.org/book".parse::<Url>()?.into()),
                ..Default::default()
            },
        };

        let mut pages = Pages::new(mdbook_markdown_options());

        for doc in TEST_DOCUMENTS {
            pages.insert(doc.url(), doc.content)?;
        }

        env.resolve(&mut pages);

        Ok(Fixture { env, pages })
    })()
    .unwrap()
});

fn assert_output(doc: TestDocument) -> Result<()> {
    let output = FIXTURE.pages.emit(&doc.url())?;
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
        fn test_output(#[case] doc: TestDocument) -> Result<()> {
            assert_output(doc)
        }
    };
}

test_output!["tests/links.md", "tests/headings.md",];

macro_rules! matcher {
    ( $pattern:pat ) => {
        |status: &LinkStatus| matches!(status, $pattern)
    };
}

#[rstest]
#[case("_stderr.ignored", matcher!(LinkStatus::Ignored))]
#[case("_stderr.published", matcher!(LinkStatus::Published))]
#[case("_stderr.rewritten", matcher!(LinkStatus::Rewritten))]
#[case("_stderr.permalink", matcher!(LinkStatus::Permalink))]
#[case("_stderr.not-checked-in", matcher!(LinkStatus::PathNotCheckedIn))]
#[case("_stderr.no-such-path", matcher!(LinkStatus::NoSuchPath))]
#[case("_stderr.no-such-fragment", matcher!(LinkStatus::NoSuchFragment))]
#[case("_stderr.link-error", matcher!(LinkStatus::Error(..)))]
fn test_stderr(#[case] name: &str, #[case] matcher: impl Fn(&LinkStatus) -> bool) -> Result<()> {
    let Fixture { env, pages } = &*FIXTURE;
    let report = env
        .report_issues(pages, matcher)
        .level(LevelFilter::Debug)
        .names(|url| env.rel_path(url))
        .colored(false)
        .logging(false)
        .build()
        .to_report();
    portable_snapshots!().test(|| insta::assert_snapshot!(name, report))?;
    Ok(())
}
