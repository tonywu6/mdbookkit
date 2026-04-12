use std::fmt::Write;

use anyhow::{Context, Result};
use lol_html::{HtmlRewriter, element};
use mdbook_markdown::pulldown_cmark::{Event, LinkType, Parser, Tag};
use mdbookkit::markdown::default_markdown_options;
use tap::Pipe;
use url::Url;

use mdbookkit_testing::{
    snapbox::{assert_data_eq, file},
    test_mdbook,
};

test_mdbook![
    rustdoc(RustDoc),
    exit(0),
    stderr.svg = file!["rustdoc/stderr/data.svg": TermSvg],
    stderr.txt = file!["rustdoc/stderr/data.txt"],
    rendered = [
        file!["rustdoc/out/basic.md"],
        file!["rustdoc/out/ignored.md"]
    ],
];

#[test]
fn rustdoc_parity() -> Result<()> {
    let book = RustDoc::book()?;

    book.cargo("clean", book.dirs.book_dir());
    book.cargo("doc", book.dirs.book_dir()).assert().success();

    let base = format! { "https://docs.rs/{}/0.1.0/{}/", book.dirs.name, book.dirs.name }
        .parse::<Url>()?;

    let mut upstream = String::new();
    let mut expected = String::new();

    for case in book.rendered {
        let html = (book.dirs.rel_path(&case).unwrap())
            .with_extension("")
            .join("index.html");

        writeln!(upstream, "# {html}\n")?;
        writeln!(expected, "# {html}\n")?;

        let base = base.join(html.as_str())?;

        let html = (book.dirs.book_dir())
            .join("target/doc")
            .join(book.dirs.name)
            .join(html);

        let html = std::fs::read_to_string(&html).context(html)?;

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

        let rendered = case.to_string();

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

    assert_data_eq!(upstream, expected);

    Ok(())
}
