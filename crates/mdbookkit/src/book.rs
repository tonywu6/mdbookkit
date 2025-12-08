use std::{
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use mdbook::{
    BookItem,
    book::{Book, Chapter},
    preprocess::PreprocessorContext,
};
use serde::de::DeserializeOwned;
use tap::Pipe;

pub fn book_from_stdin() -> Result<(PreprocessorContext, Book)> {
    Ok(Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(serde_json::from_str)?)
}

pub fn config_from_book<T>(config: &mdbook::Config, name: &str) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    let name = name.strip_prefix("mdbook-").unwrap_or(name);
    if let Some(config) = config.get_preprocessor(name) {
        T::deserialize(toml::Value::Table(config.clone()))?
    } else {
        Default::default()
    }
    .pipe(Ok)
}

pub fn smart_punctuation(config: &mdbook::Config) -> bool {
    config
        .get_deserialized_opt::<bool, _>("output.html.smart-punctuation")
        .unwrap_or_default()
        .unwrap_or(true)
}

pub fn iter_chapters(book: &Book) -> impl Iterator<Item = (&PathBuf, &Chapter)> {
    book.iter().filter_map(|item| {
        let BookItem::Chapter(ch) = item else {
            return None;
        };
        let Some(path) = &ch.source_path else {
            return None;
        };
        Some((path, ch))
    })
}

pub fn for_each_chapter_mut<F>(book: &mut Book, mut func: F)
where
    F: FnMut(PathBuf, &mut Chapter),
{
    book.for_each_mut(|item| {
        let BookItem::Chapter(ch) = item else { return };
        let Some(path) = &ch.source_path else { return };
        func(path.clone(), ch)
    });
}

pub fn book_into_stdout(book: &Book) -> Result<()> {
    serde_json::to_string(&book)
        .context("failed to serialize book")
        .and_then(|output| Ok(std::io::stdout().write_all(output.as_bytes())?))
        .context("failed to write book to stdout")
}
