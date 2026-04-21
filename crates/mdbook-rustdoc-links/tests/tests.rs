use std::fmt::Write;

use anyhow::{Context, Result};
use lol_html::{HtmlRewriter, element};
use mdbook_markdown::pulldown_cmark::{Event, LinkType::*, Parser, Tag};
use tap::Pipe;
use url::Url;

use mdbookkit::markdown::default_markdown_options;
use mdbookkit_testing::{
    regex::Regex,
    snapbox::{IntoData, RedactedValue, Redactions, assert_data_eq},
    test_mdbook,
};

test_mdbook![rustdoc(RustDoc), exit(0), redacted = [redacted()]];
test_mdbook![targets, exit(0), redacted = [redacted()]];
test_mdbook![packages, exit(0), redacted = [redacted()]];
test_mdbook![preludes, exit(0), redacted = [redacted()]];
test_mdbook![preludes_implicit, exit(0), redacted = [redacted()]];
test_mdbook![preludes_bin, exit(0), redacted = [redacted()]];
test_mdbook![features, exit(0), redacted = [redacted()]];
test_mdbook![cargo_customize, exit(0), redacted = [redacted()]];
test_mdbook![runner, exit(0), redacted = [redacted()]];
test_mdbook![docs_rs, exit(0), redacted = [redacted()]];
test_mdbook![workspace, exit(0), redacted = [redacted()]];
test_mdbook![workspace_deps, exit(0), redacted = [redacted()]];
test_mdbook![workspace_all, exit(0), redacted = [redacted()]];
test_mdbook![multi_stage, exit(0), redacted = [redacted()]];
test_mdbook![targets_proc_macro, exit(0), redacted = [redacted()]];
test_mdbook![packages_dev, exit(0), redacted = [redacted()]];
test_mdbook![
    manifest_dir,
    exit(0),
    redacted = [redacted()],
    manifest = "./rust"
];

test_mdbook![preludes_invalid, exit(101), redacted = [redacted()]];
test_mdbook![
    compilation_error,
    exit(101),
    env = ["CARGO_TERM_QUIET" = "true"],
    redacted = [redacted()]
];
test_mdbook![
    multi_stage_some_failed,
    exit(0),
    env = ["CARGO_TERM_QUIET" = "true"],
    redacted = [redacted()]
];
test_mdbook![
    multi_stage_all_failed,
    exit(101),
    env = ["CARGO_TERM_QUIET" = "true"],
    redacted = [redacted()]
];

#[test]
fn rustdoc_parity() -> Result<()> {
    let book = RustDoc::book()?;

    book.cargo("clean", book.path.book_dir());
    book.cargo("doc", book.path.book_dir()).assert().success();

    let base = format! { "https://docs.rs/{}/0.1.0/{}/", book.path.name, book.path.name }
        .parse::<Url>()?;

    let mut upstream = String::new();
    let mut expected = String::new();

    for page in book.path.expected_pages()? {
        let page = page?;

        writeln!(upstream, "# {}\n", page.name())?;
        writeln!(expected, "# {}\n", page.name())?;

        let html = format!("{}/index.html", page.mod_name());
        let base = base.join(&html)?.join(".")?;

        let html = (book.path.book_dir())
            .join("target/doc")
            .join(book.path.name)
            .join(html);

        let html = std::fs::read_to_string(&html)
            .context(html)
            .context("rustdoc did not emit this file")?;

        let link_ignored = |url: &Url| {
            url.scheme() != "https" ||
            // links pointing at the same directory as the page itself
            // are likely [inline](links) that are broken
            url.as_str().starts_with(base.as_str())
        };

        lol_html::Settings {
            element_content_handlers: vec![element!(".top-doc a", |elem| {
                if elem.get_attribute("class").as_deref() == Some("doc-anchor") {
                    return Ok(());
                }
                let Some(href) = elem.get_attribute("href") else {
                    return Ok(());
                };
                let href = base.join(&href)?;
                if link_ignored(&href) {
                    return Ok(());
                }
                let title = elem.get_attribute("title").unwrap_or_default();
                writeln!(upstream, "{href} {title:?}",)?;
                Ok(())
            })],
            ..Default::default()
        }
        .pipe(|cb| HtmlRewriter::new(cb, |_: &[u8]| ()))
        .pipe(|mut wr| wr.write(html.as_bytes()).and_then(|_| wr.end()))?;

        let rendered = page.expected().to_string();

        for event in Parser::new_ext(&rendered, default_markdown_options()) {
            if let Event::Start(Tag::Link {
                dest_url,
                title,
                link_type: Inline | Reference | Collapsed | Shortcut,
                ..
            }) = event
                && let Ok(url) = dest_url.parse::<Url>()
                && !link_ignored(&url)
            {
                writeln!(expected, "{url} {:?}", &*title)?;
            }
        }

        writeln!(upstream)?;
        writeln!(expected)?;
    }

    let redactions = {
        let mut redactions = Redactions::new();
        for (k, v) in redacted() {
            redactions.insert(k, v)?;
        }
        redactions.insert(
            "[CRATE]",
            r"https://docs\.rs/(?<redacted>[a-z_-]+/[0-9.]+)/".parse::<Regex>()?,
        )?;
        redactions
    };

    let upstream = redactions.redact(&upstream).into_data().raw();
    let expected = redactions.redact(&expected).into_data().raw();

    assert_data_eq!(upstream, expected);

    Ok(())
}

fn redacted() -> Vec<(&'static str, RedactedValue)> {
    vec![
        (
            "[RUST_VERSION]",
            Regex::new(
                r"https://doc\.rust-lang\.org/(?<redacted>nightly|1\.\d+\.\d+)/(core|alloc|std)/",
            )
            .unwrap()
            .into(),
        ),
        (
            "[TEMP_DIR]",
            Regex::new(r"\.tmp[A-Za-z0-9]+").unwrap().into(),
        ),
        (
            "[BUILD_HASH]",
            Regex::new(r"/lib.+?-(?<redacted>[a-z0-9]+?)\.rmeta")
                .unwrap()
                .into(),
        ),
    ]
}
