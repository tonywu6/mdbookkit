use anyhow::Result;
use log::LevelFilter;
use tap::Pipe;

use mdbookkit::{
    bin::link_forever::{git::GitHubPermalink, Environment, Pages},
    markdown::mdbook_markdown,
};
use util_testing::{portable_snapshots, test_document, CARGO_WORKSPACE_DIR};

#[test]
fn test_links() -> Result<()> {
    let book_src = CARGO_WORKSPACE_DIR.join("crates/mdbookkit/tests/")?;
    let vcs_root = CARGO_WORKSPACE_DIR.join("crates/mdbookkit/")?;
    let get_link = GitHubPermalink::new("lorem", "ipsum", "dolor")?.pipe(Box::new);

    let env = Environment {
        book_src,
        vcs_root,
        get_link,
    };

    let mut pages = Pages::new(mdbook_markdown());

    let main_page = test_document!("tests/link-forever.md");
    let side_page = test_document!("tests/ra-known-quirks.md");

    pages.insert(main_page.file.clone(), main_page.source)?;
    pages.insert(side_page.file.clone(), side_page.source)?;

    env.resolve(&mut pages);

    let output = pages.emit(&main_page.file)?;

    let name = main_page.name.clone();

    portable_snapshots!().test(|| insta::assert_snapshot!(format!("{name}"), output))?;

    let report = env
        .report(&pages)
        .level(LevelFilter::Debug)
        .names(|url| env.rel_path(url))
        .colored(false)
        .logging(false)
        .build()
        .to_report();

    portable_snapshots!().test(|| insta::assert_snapshot!(format!("{name}.stderr"), report))?;

    Ok(())
}
