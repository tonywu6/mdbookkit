//! Postprocess mdBook HTML output to add OpenGraph metadata, for social images, etc.
//!
//! mdBook doesn't support frontmatters yet, so this cannot be a preprocessor.

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use glob::glob;
use lol_html::{
    element, html_content::ContentType, rewrite_str, text, HtmlRewriter, RewriteStrSettings,
    Settings,
};
use minijinja::Environment;
use serde::Deserialize;
use serde_json::json;
use tap::{Pipe, Tap};
use url::Url;

fn main() -> Result<()> {
    let Program { root_dir } = Program::parse();

    let jinja =
        Environment::new().tap_mut(|env| env.add_template("index.html", OPEN_GRAPH).unwrap());

    let root_dir = std::fs::canonicalize(root_dir)?
        .pipe(Url::from_directory_path)
        .unwrap();

    let book_toml_path = root_dir.join("book.toml")?;

    let book_toml = book_toml_path
        .path()
        .pipe(std::fs::read_to_string)?
        .pipe_deref(toml::from_str::<BookToml>)?;

    let src_dir = book_toml.book.src.as_deref().unwrap_or("src");
    let src_dir = root_dir.join(&format!("{src_dir}/"))?;

    let out_dir = book_toml.build.build_dir.as_deref().unwrap_or("book");
    let out_dir = root_dir.join(&format!("{out_dir}/"))?;

    let metadata = book_toml
        .metadata
        .socials
        .0
        .into_iter()
        .map(|(prefix, metadata)| -> Result<(_, PageMetadata)> {
            let image = match metadata.image {
                None => return Ok((prefix, metadata)),
                Some(image) => image,
            };
            let image = book_toml_path.join(&image)?;
            let image = src_dir
                .make_relative(&image)
                .context("failed to make relative path to image")?;
            let image = book_toml.metadata.base_url.join(&image)?;
            let metadata = PageMetadata {
                title: metadata.title,
                image: Some(image.to_string()),
            };
            Ok((prefix, metadata))
        })
        .collect::<Result<Vec<_>>>()?
        .tap_mut(|metadata| metadata.sort_by(|(p1, _), (p2, _)| p1.cmp(p2)));

    let theme_color = book_toml
        .metadata
        .default_theme_color
        .as_deref()
        .unwrap_or("#00000000");

    for path in glob(out_dir.join("**/*.html")?.path())? {
        let url = Url::from_file_path(path?).unwrap();

        let pathname = out_dir
            .make_relative(&url)
            .context("failed to get page pathname")?;

        let src_path = src_dir.join(&pathname.replace(".html", ".md"))?;

        if !std::fs::exists(src_path.path()).unwrap_or(false) {
            continue;
        }

        let html = std::fs::read_to_string(url.path())?;

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
                ],
                ..Default::default()
            }
            .pipe(|settings| HtmlRewriter::new(settings, |_: &[u8]| ()))
            .pipe(|mut wr| wr.write(html.as_bytes()).and(Ok(wr)))?
            .pipe(|wr| wr.end())?;

            (collapse_whitespace(title), collapse_whitespace(description))
        };

        let pathname = pathname
            .replace("index.html", "")
            .replace(".html", "")
            .pipe(|p| format!("/{p}"));

        let title = metadata
            .iter()
            .filter_map(|(prefix, metadata)| {
                let title = metadata.title.as_ref()?;
                if pathname.starts_with(prefix) && &pathname != prefix
                // pathname != prefix because subroute index page
                // should already have a sensible title
                {
                    Some(title.as_str())
                } else {
                    None
                }
            })
            .chain(std::iter::once(og_title.as_str()))
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");

        let og_image = metadata.iter().rev().find_map(|(prefix, metadata)| {
            if !pathname.starts_with(prefix) {
                None
            } else {
                metadata.image.as_ref()?.parse::<Url>().ok()
            }
        });

        let og_url = book_toml.metadata.base_url.join(&pathname[1..])?;

        let og_site_name = book_toml.book.title.as_deref();

        let ctx = json!({
            "og_title": og_title,
            "og_image": og_image,
            "og_url": og_url,
            "og_description": og_description,
            "og_site_name": og_site_name,
        });

        let html = RewriteStrSettings {
            element_content_handlers: vec![
                element!("title", |elem| {
                    elem.set_inner_content(&title, ContentType::Text);
                    Ok(())
                }),
                element!(r#"meta[property^="og"]"#, |elem| {
                    elem.remove();
                    Ok(())
                }),
                element!(r#"meta[name="description"]"#, |elem| {
                    let meta = jinja.get_template("index.html").unwrap().render(&ctx)?;
                    elem.set_attribute("content", &og_description)?;
                    elem.before(&meta, ContentType::Html);
                    Ok(())
                }),
                element!(r#"meta[name="theme-color"]"#, |elem| {
                    elem.set_attribute("content", theme_color)?;
                    Ok(())
                }),
            ],
            ..Default::default()
        }
        .pipe(|settings| rewrite_str(&html, settings))?;

        std::fs::write(url.path(), html)?;
    }

    Ok(())
}

static OPEN_GRAPH: &str = r#"
    <meta property="og:type"        content="article">
    <meta property="og:title"       content="{{ og_title }}">
    <meta property="og:url"         content="{{ og_url }}">
    <meta property="og:image"       content="{{ og_image }}">
    <meta property="og:description" content="{{ og_description }}">
    <meta property="og:site_name"   content="{{ og_site_name }}">
"#;

#[derive(Parser)]
struct Program {
    root_dir: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BookToml {
    book: BookConfig,
    build: BuildConfig,
    #[serde(rename = "_metadata")]
    metadata: MetadataConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BookConfig {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    src: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BuildConfig {
    #[serde(default)]
    build_dir: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct MetadataConfig {
    base_url: Url,
    #[serde(default)]
    default_theme_color: Option<String>,
    #[serde(default)]
    socials: Socials,
}

#[derive(Deserialize, Default)]
struct Socials(HashMap<String, PageMetadata>);

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PageMetadata {
    title: Option<String>,
    image: Option<String>,
}

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
