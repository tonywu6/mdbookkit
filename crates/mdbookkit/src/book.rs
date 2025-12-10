use std::{
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use mdbook_markdown::pulldown_cmark::Options as MarkdownOptions;
use mdbook_preprocessor::{
    PreprocessorContext,
    book::{Book, BookItem, Chapter},
    config::{Config as MDBookConfig, HtmlConfig},
};
use serde::Deserialize;
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
    Ok(serde_json::from_str(&string_from_stdin()?)?)
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

    fn to_stdout(&self) -> Result<()>;
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

    fn to_stdout(&self) -> Result<()> {
        serde_json::to_string(&self)
            .context("failed to serialize book")
            .and_then(|output| Ok(std::io::stdout().write_all(output.as_bytes())?))
            .context("failed to write book to stdout")
    }
}
