use std::{
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::Options as MarkdownOptions;
use mdbook_preprocessor::{
    PreprocessorContext,
    book::{Book, BookItem, Chapter},
    config::{Config as MDBookConfig, HtmlConfig},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tap::Pipe;

use crate::markdown::default_markdown_options;

pub fn string_from_stdin() -> Result<String> {
    Ok(Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?)
}

/// This uses [`serde_json::from_str`] whereas [`mdbook_preprocessor::parse_input`] uses
/// [`serde_json::from_reader`], which could be slow.
pub fn book_from_stdin() -> Result<(PreprocessorContext, Book)> {
    let input = string_from_stdin()?;
    match serde_json::from_str(&input) {
        Ok(book) => Ok(book),
        Err(err) => {
            if !err.is_data() {
                Err(err)?
            } else {
                patch_mdbook_input(input, err)
            }
        }
    }
}

pub trait BookConfigHelper {
    fn preprocessor<'de, T>(&self, name: &str) -> Result<T>
    where
        T: Deserialize<'de> + Default;

    fn markdown_options(&self) -> MarkdownOptions;
}

impl BookConfigHelper for MDBookConfig {
    fn preprocessor<'de, T>(&self, name: &str) -> Result<T>
    where
        T: Deserialize<'de> + Default,
    {
        let name = name.strip_prefix("mdbook-").unwrap_or(name);
        let name = format!("preprocessor.{name}");
        Ok(self.get::<T>(&name)?.unwrap_or_default())
    }

    fn markdown_options(&self) -> MarkdownOptions {
        let HtmlConfig {
            smart_punctuation,
            definition_lists,
            admonitions,
            ..
        } = self
            .get::<HtmlConfig>("output.html")
            .unwrap_or_default()
            .unwrap_or_default();
        let mut options = default_markdown_options();
        if admonitions {
            options.insert(MarkdownOptions::ENABLE_GFM);
        }
        if smart_punctuation {
            options.insert(MarkdownOptions::ENABLE_SMART_PUNCTUATION);
        }
        if definition_lists {
            options.insert(MarkdownOptions::ENABLE_DEFINITION_LIST);
        }
        options
    }
}

pub trait BookHelper {
    fn iter_chapters(&self) -> impl Iterator<Item = (&PathBuf, &Chapter)>;

    fn to_stdout(self, ctx: &PreprocessorContext) -> Result<()>;
}

impl BookHelper for Book {
    fn iter_chapters(&self) -> impl Iterator<Item = (&PathBuf, &Chapter)> {
        self.iter().filter_map(|item| {
            let BookItem::Chapter(ch) = item else {
                return None;
            };
            let Some(path) = &ch.source_path else {
                return None;
            };
            Some((path, ch))
        })
    }

    fn to_stdout(self, ctx: &PreprocessorContext) -> Result<()> {
        let output = if ctx.mdbook_version.starts_with("0.4.") {
            patch_mdbook_output_0_4(self)?
        } else {
            serde_json::to_string(&self).context("failed to serialize mdbook output")?
        };
        std::io::stdout()
            .write_all(output.as_bytes())
            .context("failed to write mdbook output")
    }
}

fn patch_mdbook_input(
    input: String,
    error: serde_json::Error,
) -> Result<(PreprocessorContext, Book)> {
    let (mut ctx, mut book): (Value, Value) = serde_json::from_str(&input)?;

    match ctx.get("mdbook_version") {
        Some(Value::String(version)) => {
            if !version.starts_with("0.4.") && !version.starts_with("0.5.") {
                bail!("unsupported mdbook version {version}; supported versions are 0.4, 0.5")
            }
        }
        _ => return Err(error)?,
    }

    // 0.4 -> 0.5
    if let Some(conf) = ctx
        .pointer_mut("/config/book")
        .and_then(|val| val.as_object_mut())
    {
        conf.remove("multilingual");
    }

    // 0.4 -> 0.5
    if let Some(book) = book.as_object_mut()
        && let Some(sections) = book.remove("sections")
    {
        book.insert("items".into(), sections);
    }

    Ok(serde_json::from_value(json!([ctx, book]))?)
}

fn patch_mdbook_output_0_4(book: Book) -> Result<String> {
    let mut book = serde_json::to_value(book)?;

    if let Some(book) = book.as_object_mut() {
        if let Some(sections) = book.remove("items") {
            book.insert("sections".into(), sections);
        }
        book.insert("__non_exhaustive".into(), Value::Null);
    }

    Ok(serde_json::to_string(&book)?)
}
