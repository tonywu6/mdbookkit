use anyhow::Result;
use log::LevelFilter;
use tap::Pipe;

use url::Url;

use mdbookkit::{
    bin::link_forever::{Config, Environment, GitHubPermalink, Pages},
    logging::ConsoleLogger,
    markdown::mdbook_markdown,
};
use util_testing::{portable_snapshots, test_document, CARGO_WORKSPACE_DIR};

#[test]
fn test_links() -> Result<()> {
    ConsoleLogger::install("link-forever");

    let env = Environment {
        book_src: CARGO_WORKSPACE_DIR.join("crates/mdbookkit/")?,
        vcs_root: CARGO_WORKSPACE_DIR.clone(),
        fmt_link: GitHubPermalink::new("lorem", "ipsum", "dolor").pipe(Box::new),
        markdown: mdbook_markdown(),
        config: Config {
            book_url: Some("https://example.org/book".parse::<Url>()?.into()),
            ..Default::default()
        },
    };

    let mut pages = Pages::new(mdbook_markdown());

    let tests = [
        test_document!("tests/ra-known-quirks.md"), // only for providing anchors
        test_document!("tests/link-forever.md"),
        test_document!("../README.md"),
    ];

    for page in tests.iter() {
        pages.insert(page.file.clone(), page.source)?;
    }

    env.resolve(&mut pages);

    for page in tests {
        let output = pages.emit(&page.file)?;
        let name = page.name;
        portable_snapshots!().test(|| insta::assert_snapshot!(format!("{name}"), output))?;
    }

    let report = env
        .report(&pages)
        .level(LevelFilter::Debug)
        .names(|url| env.rel_path(url))
        .colored(false)
        .logging(false)
        .build()
        .to_report();

    portable_snapshots!().test(|| insta::assert_snapshot!("_stderr", report))?;

    Ok(())
}
