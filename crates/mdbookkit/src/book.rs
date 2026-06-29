use std::{
    borrow::{Borrow, Cow},
    hash::Hash,
    io::{Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use mdbook_markdown::pulldown_cmark::Options as MarkdownOptions;
use mdbook_preprocessor::{
    PreprocessorContext,
    book::{Book, BookItem, Chapter},
    config::{self, HtmlConfig},
};
use serde::{Deserialize, Deserializer, de::IntoDeserializer};
use serde_json::{Value, json};
use tap::Pipe;
use tracing::warn;
use url::Url;

use crate::{
    config::FeatureGated, emit_debug, error::WithDebugContext, markdown::default_markdown_options,
    url::UrlFromPath,
};

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

#[allow(clippy::result_unit_err)]
pub trait PreprocessorHelper {
    fn book_toml(&self) -> BookToml<'_>;

    fn markdown_options(&self) -> MarkdownOptions;

    fn book_dir(&self) -> Result<PathBuf>;

    fn page_dir(&self) -> Result<PathBuf>;

    fn for_each_page<'a, F, E>(&self, book: &'a Book, func: F) -> Result<(), E>
    where
        F: FnMut(Url, &'a str) -> Result<(), E>;

    fn for_each_page_mut<F, E>(&self, book: &mut Book, func: F) -> Result<(), E>
    where
        F: FnMut(Url, &mut String) -> Result<(), E>;

    fn print(&self, book: Book) -> Result<()>;
}

impl PreprocessorHelper for PreprocessorContext {
    fn book_toml(&self) -> BookToml<'_> {
        BookToml::from_ctx(self)
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

    fn book_dir(&self) -> Result<PathBuf> {
        let path = &self.root;
        let path = (path.canonicalize())
            .with_path_debug(path)
            .context("could not access the root directory of the book")?;
        Ok(path)
    }

    fn page_dir(&self) -> Result<PathBuf> {
        Ok(self.book_dir()?.join(&self.config.book.src))
    }

    fn for_each_page<'a, F, E>(&self, book: &'a Book, mut func: F) -> Result<(), E>
    where
        F: FnMut(Url, &'a str) -> Result<(), E>,
    {
        for item in book.iter() {
            let BookItem::Chapter(ch) = item else {
                continue;
            };
            let Some(path) = &ch.source_path else {
                continue;
            };
            func(page_url(self, path), &ch.content)?;
        }
        Ok(())
    }

    fn for_each_page_mut<F, E>(&self, book: &mut Book, mut func: F) -> Result<(), E>
    where
        F: FnMut(Url, &mut String) -> Result<(), E>,
    {
        let mut result = Ok(());
        book.for_each_chapter_mut(|ch| {
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
            result = func(page_url(self, path), content);
        });
        result
    }

    fn print(&self, book: Book) -> Result<()> {
        let output = if self.mdbook_version.starts_with("0.4.") {
            patch_mdbook_output_0_4(book)
        } else {
            serde_json::to_string(&book).map_err(Into::into)
        }
        .context("failed to serialize mdBook output")?;
        std::io::stdout()
            .write_all(output.as_bytes())
            .context("failed to write mdBook output")
    }
}

fn page_dir(ctx: &PreprocessorContext) -> PathBuf {
    ctx.root.join(&ctx.config.book.src)
}

fn page_url(ctx: &PreprocessorContext, path: &Path) -> Url {
    let base = page_dir(ctx);
    let path = base.join(path);
    path.file_to_url()
}

#[derive(Debug)]
pub struct BookToml<'a> {
    config: Cow<'a, config::Config>,
    source: BookTomlSource,
}

#[derive(Debug)]
enum BookTomlSource {
    Read(String),
    Path(PathBuf),
}

macro_rules! preprocessor_table {
    (preprocessor.$name:expr) => {
        format!("preprocessor.{}", preprocessor_table!($name))
    };
    ($name:expr) => {
        $name.strip_prefix("mdbook-").unwrap_or($name)
    };
}

impl<'a> BookToml<'a> {
    fn from_ctx(ctx: &'a PreprocessorContext) -> Self {
        Self {
            config: Cow::Borrowed(&ctx.config),
            source: BookTomlSource::Path(ctx.root.join("book.toml")),
        }
    }

    fn load_source(&mut self) -> Result<&str> {
        let path = match self.source {
            BookTomlSource::Path(ref path) => path,
            BookTomlSource::Read(ref source) => return Ok(source),
        };

        let source = std::fs::read_to_string(path)
            .with_path_debug(path)
            .context("failed to reload config from file")?;

        let mut config = config::Config::from_str(&source)
            .with_path_debug(path)
            .context("failed to reload config from file")?;

        config
            .update_from_env()
            .context("failed to update config from environment variables")?;

        self.config = Cow::Owned(config);
        self.source = BookTomlSource::Read(source);

        self.load_source()
    }

    fn read_by_path<T>(&mut self, path: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let deserializer = match self.config.get::<toml::Value>(path)? {
            None => return Ok(None),
            Some(value) => value,
        }
        .toml_deserializer(path);

        let error = match deserializer.deserialize() {
            Ok(config) => return Ok(Some(config)),
            Err(error) => error,
        };

        if let Ok(source) = self.load_source().or_else(emit_debug!())
            && let Ok(error) = recover_toml_error::<T>(source, path).or_else(emit_debug!())
        {
            Err(error)?
        } else {
            Err(error)?
        }
    }

    pub fn inner(&self) -> &config::Config {
        &self.config
    }

    pub fn with_source(mut self) -> Self {
        self.load_source().or_else(emit_debug!()).ok();
        self
    }

    pub fn preprocessor<T>(&mut self, names: &[&str]) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        for (idx, name) in names.iter().enumerate() {
            let name = preprocessor_table!(preprocessor.name);
            if let Some(value) = self.read_by_path(&name)? {
                if idx != 0 {
                    warn! {
                        "the book.toml section [{deprecated}] is deprecated, \
                        use [{recommended}] instead.",
                        deprecated  = name,
                        recommended = preprocessor_table!(preprocessor.names[0])
                    };
                }
                return Ok(value);
            }
        }
        Ok(None)
    }

    pub fn html_config<T>(&mut self, key: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.read_by_path::<T>(&format!("output.html.{key}"))
    }
}

impl FromStr for BookToml<'static> {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            config: Cow::Owned(s.parse()?),
            source: BookTomlSource::Read(s.to_owned()),
        })
    }
}

fn recover_toml_error<T>(source: &str, path: &str) -> Result<toml::de::Error>
where
    T: for<'de> Deserialize<'de>,
{
    let table = toml::de::DeTable::parse(source)?;
    let table = toml::Spanned::new(table.span(), toml::de::DeValue::Table(table.into_inner()));
    let deserializer = path
        .split('.')
        .try_fold(table, |parent, key| {
            if let toml::de::DeValue::Table(mut table) = parent.into_inner() {
                table.remove(key)
            } else {
                None
            }
        })
        .context("value not defined in source")?
        .toml_deserializer(path);

    if let Err(mut error) = deserializer.deserialize::<T>() {
        error.set_input(Some(source));
        Ok(error)
    } else {
        bail!("parsing from TOML source did not error")
    }
}

trait BookTomlDeserializer<'de> {
    type Deserializer: Deserializer<'de, Error = toml::de::Error>;
    fn toml_deserializer(self, path: &str) -> FeatureGated<Self::Deserializer>;
}

impl<'de> BookTomlDeserializer<'de> for toml::Value {
    type Deserializer = toml::Value;

    fn toml_deserializer(mut self, path: &str) -> FeatureGated<Self::Deserializer> {
        let mut unstable = None;
        if path.starts_with("preprocessor.")
            && let toml::Value::Table(ref mut table) = self
        {
            if let Some(flag) = table.remove("unstable-features") {
                unstable = Some(flag);
            }
            remove_builtin_options(table);
        }
        FeatureGated {
            unstable,
            deserializer: self.into_deserializer(),
        }
    }
}

impl<'de> BookTomlDeserializer<'de> for toml::Spanned<toml::de::DeValue<'de>> {
    type Deserializer = toml::de::ValueDeserializer<'de>;

    fn toml_deserializer(mut self, path: &str) -> FeatureGated<Self::Deserializer> {
        let mut unstable = None;
        if path.starts_with("preprocessor.")
            && let toml::de::DeValue::Table(table) = self.as_mut()
        {
            if let Some(flag) = table.remove("unstable-features") {
                unstable = Some(flag.into_deserializer());
            }
            remove_builtin_options(table);
        }
        FeatureGated {
            unstable,
            deserializer: self.into_deserializer(),
        }
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

fn patch_mdbook_input(
    input: String,
    error: serde_json::Error,
) -> Result<(PreprocessorContext, Book)> {
    let (mut ctx, mut book): (Value, Value) = serde_json::from_str(&input)?;

    match ctx.get("mdbook_version") {
        Some(Value::String(version)) => {
            if !version.starts_with("0.4.") && !version.starts_with("0.5.") {
                bail! { "unsupported mdBook version {version}; \
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

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn should_emit_issues(_: &PreprocessorContext) -> bool {
    true
}

#[cfg(debug_assertions)]
#[inline(always)]
pub fn should_emit_issues(ctx: &PreprocessorContext) -> bool {
    use crate::{env::TruthyStr, env_var};

    env_var!(NEXTEST);

    if NEXTEST.truthy().is_some() {
        ctx.renderer == "markdown"
    } else {
        true
    }
}
