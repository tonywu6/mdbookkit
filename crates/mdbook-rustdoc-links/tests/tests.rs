use std::fmt::Write;

use anyhow::{Context, Result};
use lol_html::{HtmlRewriter, element};
use mdbook_markdown::pulldown_cmark::{Event, LinkType::*, Parser, Tag};
use tap::Pipe;
use url::Url;

use mdbookkit::markdown::default_markdown_options;
use mdbookkit_testing::{
    TestBook,
    regex::Regex,
    snapbox::{IntoData, RedactedValue, Redactions, assert_data_eq},
    test_mdbook,
};

macro_rules! test_case {
    [$name:ident, $($args:tt)+] => {
        mod $name {
            use super::*;
            test_mdbook![$name, $($args)+, redacted = [redacted()]];
        }
        #[test]
        fn $name() -> Result<()> {
            run_test($name::$name()?, ".")
        }
    };
}

test_case![rustdoc, exit(0)];
test_case![targets, exit(0)];
test_case![packages, exit(0)];
test_case![preludes, exit(0)];
test_case![preludes_implicit, exit(0)];
test_case![preludes_bin, exit(0)];
test_case![features, exit(0)];
test_case![cargo_customize, exit(0)];
test_case![runner, exit(0)];
test_case![docs_rs, exit(0)];
test_case![workspace, exit(0)];
test_case![workspace_deps, exit(0)];
test_case![workspace_all, exit(0)];
test_case![multi_stage, exit(0)];
test_case![targets_proc_macro, exit(0)];
test_case![packages_dev, exit(0)];
test_case![diagnostics_order, exit(0)];

test_case![preludes_invalid, exit(101)];
test_case![compilation_error, exit(101)];
test_case![multi_stage_some_failed, exit(0)];
test_case![multi_stage_all_failed, exit(101)];
test_case![runner_bad_command, exit(101)];
test_case![runner_not_found, exit(101)];
test_case![manifest_invalid, exit(101)];
test_case![packages_invalid, exit(101)];
test_case![deserialize_workspace, exit(101)];
test_case![deserialize_package, exit(101)];
test_case![
    debug_logs,
    exit(0),
    env = [
        "MDBOOK_LOG" = "warn,mdbook_rustdoc_links=trace",
        "MDBOOKKIT_TERM_GRAPHICAL" = "",
        "CARGO_TERM_QUIET" = "true"
    ]
];

test_case![book_getting_started, exit(0)];
test_case![book_link_syntax_escape_generics, exit(0)];
test_case![book_link_syntax_unsupported_generics, exit(0)];

#[test]
fn manifest_dir() -> Result<()> {
    test_mdbook![manifest_dir, exit(0)];
    run_test(manifest_dir()?, "rust")
}

#[test]
fn rustdoc_parity() -> Result<()> {
    let book = rustdoc::rustdoc()?;

    book.cargo("clean", book.path.book_dir()).assert().success();
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

fn run_test(book: TestBook, manifest_dir: &str) -> Result<()> {
    let manifest_dir = book.path.book_dir().join(manifest_dir);
    let _ = book.cargo("clean", manifest_dir).output();
    book.run()
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
        (
            "[CARGO_STDERR]",
            Regex::new(
                r"--- cargo stderr\n       (?<redacted>(.|\s|\n)+?)\n       error: could not",
            )
            .unwrap()
            .into(),
        ),
    ]
}
