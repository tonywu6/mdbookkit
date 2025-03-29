use anyhow::{anyhow, Result};
use serde::Deserialize;
use tap::Pipe;

#[cfg_attr(feature = "common-cli", derive(clap::ValueEnum))]
#[derive(Deserialize, Debug, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ErrorHandling {
    /// Fail if the environment variable `CI` is set to a value other than `0`.
    /// Environments like GitHub Actions configure this automatically.
    #[default]
    #[serde(rename = "ci")]
    #[cfg_attr(feature = "common-cli", clap(name = "ci"))]
    Env,

    /// Fail as long as there are unresolved items, even in local use.
    Always,
}

impl ErrorHandling {
    pub fn check(&self, level: log::Level) -> Result<()> {
        match level {
            log::Level::Error => Err(anyhow!("preprocessor has errors")),
            log::Level::Warn => match self {
                Self::Always => {
                    anyhow!("treating warnings as errors because fail-on-unresolved is \"always\"")
                        .context("preprocessor has errors")
                        .pipe(Err)
                }
                Self::Env => {
                    let ci = std::env::var("CI").unwrap_or("".into());
                    if matches!(ci.as_str(), "" | "0" | "false") {
                        return Ok(());
                    }
                    anyhow!("treating warnings as errors because fail-on-unresolved is \"ci\" and CI={ci}")
                        .context("preprocessor has errors")
                        .pipe(Err)
                }
            },
            _ => Ok(()),
        }
    }
}

#[cfg(feature = "common-cli")]
pub use book::*;
#[cfg(feature = "common-cli")]
mod book {
    use std::{io::Read, path::PathBuf};

    use anyhow::{Context, Result};
    use mdbook::{
        book::{Book, Chapter},
        preprocess::PreprocessorContext,
        BookItem,
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
        if let Some(config) = config.get_preprocessor(name) {
            T::deserialize(toml::Value::Table(config.clone()))
                .context("failed to read preprocessor config from book.toml")?
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
}
