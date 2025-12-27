use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    path::PathBuf,
};

use anyhow::{Context, Result};
use glob::glob;
use lol_html::{
    HtmlRewriter, RewriteStrSettings, Settings, element, html_content::ContentType, rewrite_str,
    text,
};
use minijinja::Environment;
use serde::Deserialize;
use serde_json::json;
use tap::{Pipe, Tap};
use tracing::{debug, error, info, info_span, trace};
use url::Url;

use mdbookkit::{error::OnWarning, url::UrlFromPath};

pub fn run(root_dir: PathBuf) -> Result<()> {
    let jinja =
        Environment::new().tap_mut(|env| env.add_template("index.html", OPEN_GRAPH).unwrap());

    let root_dir = std::fs::canonicalize(root_dir)?.to_directory_url();

    let book_toml_path = root_dir.join("book.toml")?;

    let book_toml = book_toml_path
        .path()
        .pipe(std::fs::read_to_string)?
        .pipe_deref(toml::from_str::<BookToml>)?;

    debug!("{book_toml:#?}");

    let src_dir = book_toml.book.src.as_deref().unwrap_or("src");
    let src_dir = root_dir.join(&format!("{src_dir}/"))?;

    let out_dir = book_toml.build.build_dir.as_deref().unwrap_or("book");
    let out_dir = root_dir.join(&format!("{out_dir}/"))?;

    let BookToml {
        preprocessor:
            PreprocessorConfig {
                permalinks: PermalinksConfig { book_url },
                ..
            },
        ..
    } = book_toml;

    let metadata = (book_toml.preprocessor.doc.socials.0)
        .into_iter()
        .map(|(prefix, metadata)| -> Result<(_, PageMetadata)> {
            let image = match metadata.image {
                None => return Ok((prefix, metadata)),
                Some(image) => image,
            };
            let image = if let Ok(image) = image.parse::<Url>() {
                image
            } else {
                let image = book_toml_path.join(&image)?;
                let image = src_dir
                    .make_relative(&image)
                    .context("Failed to make relative path to image")?;
                book_url.join(&image)?
            };
            let metadata = PageMetadata {
                title: metadata.title,
                image: Some(image.to_string()),
            };
            Ok((prefix, metadata))
        })
        .collect::<Result<Vec<_>>>()?
        .tap_mut(|metadata| metadata.sort_by(|(p1, _), (p2, _)| p1.cmp(p2)));

    debug!("{metadata:#?}");

    let mut fragments: HashSet<Url> = HashSet::new();
    let mut book_links: HashMap<Url, HashSet<Url>> = HashMap::new();

    for path in glob(out_dir.join("**/*.html")?.path())? {
        let file_url = path?.to_file_url();
        let html = std::fs::read_to_string(file_url.path())?;

        let _span = info_span!("html").entered();

        info!(%file_url);

        let (og_title, og_description) = {
            let mut title = String::new();
            let mut description = String::new();

            Settings {
                element_content_handlers: vec![
                    text!("main > h1:first-of-type", |text| {
                        title.push_str(text.as_str());
                        Ok(())
                    }),
                    text!("main > p:first-of-type", |text| {
                        description.push_str(text.as_str());
                        Ok(())
                    }),
                    element!("[id]", |elem| {
                        if let Some(id) = elem.get_attribute("id") {
                            let url = file_url.clone().tap_mut(|u| u.set_fragment(Some(&id)));
                            fragments.insert(url);
                        }
                        Ok(())
                    }),
                ],
                ..Default::default()
            }
            .pipe(|settings| HtmlRewriter::new(settings, |_: &[u8]| ()))
            .pipe(|mut wr| wr.write(html.as_bytes()).and(Ok(wr)))?
            .pipe(|wr| wr.end())?;

            (collapse_whitespace(title), collapse_whitespace(description))
        };

        let pathname = out_dir
            .make_relative(&file_url)
            .context("Failed to get page pathname")?
            .replace("index.html", "")
            .replace(".html", "")
            .pipe(|p| format!("/{p}"));

        let suffix = metadata
            .iter()
            .filter_map(|(prefix, metadata)| {
                let title = metadata.title.as_ref()?;
                // pathname != prefix because subroute index page
                // should already have a sensible title
                if pathname.starts_with(prefix) && &pathname != prefix {
                    Some(title.as_str())
                } else {
                    None
                }
            })
            .rev()
            .collect::<Vec<_>>();

        let og_image = metadata.iter().rev().find_map(|(prefix, metadata)| {
            if !pathname.starts_with(prefix) {
                None
            } else {
                metadata.image.as_ref()?.parse::<Url>().ok()
            }
        });

        let og_url = book_url.join(&pathname[1..])?;

        let og_site_name = book_toml.book.title.as_deref();

        let ctx = json!({
            "og_title": og_title,
            "og_image": og_image,
            "og_url": og_url,
            "og_description": og_description,
            "og_site_name": og_site_name,
        });

        debug!(?ctx);

        let html = RewriteStrSettings {
            element_content_handlers: vec![
                element!("title", |elem| {
                    let title = suffix.iter().fold(og_title.clone(), |mut out, suffix| {
                        write!(&mut out, " | {suffix}").and(Ok(out)).unwrap()
                    });
                    trace!(title);
                    elem.set_inner_content(&title, ContentType::Text);
                    Ok(())
                }),
                element!(r#"img[src]"#, |elem| {
                    let src = elem.get_attribute("src").unwrap();
                    let src = file_url.join(&src)?;
                    let src = match src.scheme() {
                        "file" => src,
                        _ => return Ok(()),
                    };
                    let src = match src.to_file_path() {
                        Ok(path) => path,
                        Err(()) => return Ok(()),
                    };
                    let img = image::open(src)?;
                    elem.set_attribute("width", &img.width().to_string())?;
                    elem.set_attribute("height", &img.height().to_string())?;
                    trace!(?elem);
                    Ok(())
                }),
                element!(r#"img[src^="https://img.shields.io/"]"#, |elem| {
                    elem.set_attribute("height", "20")?;
                    elem.set_attribute("fetchpriority", "low")?;
                    trace!(?elem);
                    Ok(())
                }),
                element!(r#"meta[property^="og:"]"#, |elem| {
                    elem.remove();
                    Ok(())
                }),
                element!(r#"meta[name="description"]"#, |elem| {
                    let meta = jinja.get_template("index.html").unwrap().render(&ctx)?;
                    elem.set_attribute("content", &og_description)?;
                    elem.before(&meta, ContentType::Html);
                    Ok(())
                }),
                element!(r#"h1.menu-title"#, |elem| {
                    if let Some(suffix) = suffix.iter().nth_back(1) {
                        elem.set_inner_content(suffix, ContentType::Text);
                    }
                    Ok(())
                }),
                element!(r#"a"#, |elem| {
                    let Some(href) = elem
                        .get_attribute("href")
                        .and_then(|href| file_url.join(&href).ok())
                    else {
                        return Ok(());
                    };
                    if href.scheme() == "file" && href.fragment().is_some() {
                        if let Some(set) = book_links.get_mut(&file_url) {
                            set.insert(href);
                        } else {
                            let mut set = HashSet::default();
                            set.insert(href);
                            book_links.insert(file_url.clone(), set);
                        }
                    } else if href.origin() != book_url.origin() {
                        elem.set_attribute("target", "_blank").unwrap();
                        elem.set_attribute("rel", "noreferrer").unwrap();
                    }
                    trace!(?elem);
                    Ok(())
                }),
            ],
            ..Default::default()
        }
        .pipe(|settings| rewrite_str(&html, settings))?;

        std::fs::write(file_url.path(), html)?;
    }

    for (file_url, links) in book_links {
        for mut link in links {
            if !fragments.contains(&link)
                && let Some(id) = link.fragment()
            {
                let id = format!("#{id}");
                link.set_fragment(None);
                let src = out_dir
                    .make_relative(&file_url)
                    .unwrap()
                    .replace(".html", ".md");
                let dst = out_dir
                    .make_relative(&link)
                    .unwrap()
                    .replace(".html", ".md");
                error!("{src} references non-existent {id:?} in {dst}");
            }
        }
    }

    OnWarning::FailInCi.check()
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct BookToml {
    book: BookConfig,
    build: BuildConfig,
    preprocessor: PreprocessorConfig,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct BookConfig {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    src: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct BuildConfig {
    #[serde(default)]
    build_dir: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct PreprocessorConfig {
    permalinks: PermalinksConfig,
    doc: MetadataConfig,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct PermalinksConfig {
    book_url: Url,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct MetadataConfig {
    #[serde(default)]
    socials: Socials,
}

#[derive(Deserialize, Default, Debug)]
struct Socials(HashMap<String, PageMetadata>);

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct PageMetadata {
    title: Option<String>,
    image: Option<String>,
}

static OPEN_GRAPH: &str = r##"
    <meta property="og:type"            content="article">
    <meta property="og:title"           content="{{ og_title }}">
    <meta property="og:url"             content="{{ og_url }}">
    <meta property="og:image"           content="{{ og_image }}">
    <meta property="og:description"     content="{{ og_description }}">
    <meta property="og:site_name"       content="{{ og_site_name }}">
    <meta name="twitter:card"           content="summary_large_image">
    <meta name="twitter:title"          content="{{ og_title }}">
    <meta name="twitter:image"          content="{{ og_image }}">
    <meta name="twitter:image:alt"      content="toolkit for mdBook">
    <meta name="twitter:description"    content="{{ og_description }}">
    <meta name="theme-color"            content="#d2a6ff">
"##;

fn collapse_whitespace(src: String) -> String {
    src.chars()
        .fold(
            (String::with_capacity(src.len()), None),
            |(mut out, last), ch| {
                if matches!(ch, ' ' | '\n' | '\t') {
                    if !matches!(last, Some(' ' | '\n' | '\t') | None) {
                        out.push(' ');
                    }
                } else {
                    out.push(ch);
                }
                (out, Some(ch))
            },
        )
        .0
}
