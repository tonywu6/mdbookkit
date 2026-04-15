use std::fmt::Write;

use anyhow::{Context, Result};
use lol_html::{HtmlRewriter, element};
use mdbook_markdown::pulldown_cmark::{Event, LinkType, Parser, Tag};
use mdbookkit::markdown::default_markdown_options;
use tap::Pipe;
use url::Url;

use mdbookkit_testing::{
    regex::Regex,
    snapbox::{RedactedValue, Redactions, assert_data_eq},
    test_mdbook,
};

test_mdbook![rustdoc(RustDoc), exit(0), redacted = [redacted()]];

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
        let base = base.join(&html)?;

        let html = (book.path.book_dir())
            .join("target/doc")
            .join(book.path.name)
            .join(html);

        let html = std::fs::read_to_string(&html)
            .context(html)
            .context("rustdoc did not emit this file")?;

        lol_html::Settings {
            element_content_handlers: vec![element!(".top-doc a", |elem| {
                if elem.get_attribute("class").as_deref() == Some("doc-anchor") {
                    return Ok(());
                }
                let Some(href) = elem.get_attribute("href") else {
                    return Ok(());
                };
                let href = base.join(&href)?;
                if href.scheme() != "https" {
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
                link_type: LinkType::Inline,
                ..
            }) = event
            {
                writeln!(expected, "{dest_url} {:?}", &*title)?;
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

    let upstream = redactions.redact(&upstream);
    let expected = redactions.redact(&expected);

    assert_data_eq!(&*upstream, &*expected);

    Ok(())
}

fn redacted() -> Vec<(&'static str, RedactedValue)> {
    vec![(
        "[STABLE]",
        r"https://doc\.rust-lang\.org/(?<redacted>1\.\d+\.\d+)/(core|alloc|std)/"
            .parse::<Regex>()
            .unwrap()
            .into(),
    )]
}
