use std::{
    borrow::Borrow,
    hash::Hash,
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::Options as MarkdownOptions;
use mdbook_preprocessor::{
    PreprocessorContext,
    book::{Book, BookItem, Chapter},
    config::HtmlConfig,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tap::{Pipe, Tap};
use tracing::warn;

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

pub trait PreprocessorHelper {
    fn preprocessor<T>(&self, names: &[&str]) -> Result<T>
    where
        T: for<'de> Deserialize<'de> + Default;

    fn markdown_options(&self) -> MarkdownOptions;
}

macro_rules! preprocessor_table {
    (preprocessor.$name:expr) => {
        format!("preprocessor.{}", preprocessor_table!($name))
    };
    ($name:expr) => {
        $name.strip_prefix("mdbook-").unwrap_or($name)
    };
}

impl PreprocessorHelper for PreprocessorContext {
    fn preprocessor<T>(&self, names: &[&str]) -> Result<T>
    where
        T: for<'de> Deserialize<'de> + Default,
    {
        for (idx, name) in names.iter().enumerate() {
            if let Some(value) = try_get_options(self, name)? {
                if idx != 0 {
                    warn! {
                        "The book.toml section [{deprecated}] is deprecated. \
                        Use [{recommended}] instead.",
                        deprecated  = preprocessor_table!(preprocessor.name),
                        recommended = preprocessor_table!(preprocessor.names[0])
                    };
                }
                return Ok(value);
            }
        }
        Ok(Default::default())
    }

    fn markdown_options(&self) -> MarkdownOptions {
        let HtmlConfig {
            smart_punctuation,
            definition_lists,
            admonitions,
            ..
        } = self
            .config
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

fn try_get_options<T>(ctx: &PreprocessorContext, name: &str) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let path = preprocessor_table!(preprocessor.name);

    let table = match ctx.config.get::<toml::Table>(&path)? {
        None => return Ok(None),
        Some(table) => table,
    }
    .tap_mut(remove_builtin_options);

    let error = match T::deserialize(table) {
        Ok(options) => return Ok(Some(options)),
        Err(error) => error,
    };

    Err(recover_toml_error::<T>(ctx, name).unwrap_or(error))?
}

fn recover_toml_error<T>(ctx: &PreprocessorContext, name: &str) -> Result<toml::de::Error>
where
    T: for<'de> Deserialize<'de>,
{
    let source = std::fs::read_to_string(ctx.root.join("book.toml"))?;

    let table = toml::de::DeTable::parse(&source)?;
    let table = (|| {
        table
            .into_inner()
            .remove("preprocessor")?
            .into_inner()
            .into_table()?
            .remove(preprocessor_table!(name))
    })()
    .context("no such table")?
    .tap_mut(|table| {
        if let Some(table) = table.get_mut().as_mut_table() {
            remove_builtin_options(table);
        }
    });

    let table = toml::de::ValueDeserializer::from(table);
    if let Err(mut error) = T::deserialize(table) {
        error.set_input(Some(&source));
        Ok(error)
    } else {
        bail!("parsing from book.toml did not error")
    }
}

/// Remove mdbook's builtin preprocessor options from table before deserializing
/// so that they don't interfere with `deny_unknown_fields`. Keep in-sync with:
///
/// - <https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html>
/// - <https://github.com/rust-lang/mdBook/blob/v0.5.2/crates/mdbook-driver/src/mdbook.rs#L434-L443>
fn remove_builtin_options<K, V>(table: &mut toml::map::Map<K, V>)
where
    K: Borrow<str> + Ord + Hash,
{
    table.remove("command");
    table.remove("before");
    table.remove("after");
    table.remove("optional");
    table.remove("renderers");
}

pub trait BookHelper {
    fn iter_chapters(&self) -> impl Iterator<Item = (&PathBuf, &Chapter)>;

    fn for_each_page_mut<F, E>(&mut self, func: F) -> Result<(), E>
    where
        F: FnMut(&PathBuf, &mut String) -> Result<(), E>;

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

    fn for_each_page_mut<F, E>(&mut self, mut func: F) -> Result<(), E>
    where
        F: FnMut(&PathBuf, &mut String) -> Result<(), E>,
    {
        let mut result = Ok(());
        self.for_each_chapter_mut(|ch| {
            if result.is_err() {
                return;
            }
            let &mut Chapter {
                source_path: Some(ref path),
                ref mut content,
                ..
            } = ch
            else {
                return;
            };
            result = func(path, content);
        });
        result
    }

    fn to_stdout(self, ctx: &PreprocessorContext) -> Result<()> {
        let output = if ctx.mdbook_version.starts_with("0.4.") {
            patch_mdbook_output_0_4(self)
        } else {
            serde_json::to_string(&self).map_err(Into::into)
        }
        .context("Failed to serialize mdBook output")?;
        std::io::stdout()
            .write_all(output.as_bytes())
            .context("Failed to write mdBook output")
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
                bail! { "Unsupported mdBook version {version}; \
                supported versions are 0.4, 0.5" }
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

trait TomlTableHelper {
    type TableType;
    fn into_table(self) -> Option<Self::TableType>;
    fn as_mut_table(&mut self) -> Option<&mut Self::TableType>;
}

impl<'de> TomlTableHelper for toml::de::DeValue<'de> {
    type TableType = toml::de::DeTable<'de>;

    fn into_table(self) -> Option<Self::TableType> {
        if let Self::Table(table) = self {
            Some(table)
        } else {
            None
        }
    }

    fn as_mut_table(&mut self) -> Option<&mut Self::TableType> {
        if let Self::Table(table) = self {
            Some(table)
        } else {
            None
        }
    }
}
