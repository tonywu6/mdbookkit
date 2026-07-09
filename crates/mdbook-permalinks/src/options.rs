use std::ops::Deref;

use anyhow::{Context, Result};
use mdbook_preprocessor::PreprocessorContext;
use serde::{Deserialize, Deserializer};
use url::Url;

use mdbookkit::{
    book::{BookToml, PreprocessorHelper},
    config::{BaseUrl, UnstableFeature, ValueShorthand, value_or_vec, value_shorthand, via},
    env::is_ci,
    error::FailOnWarnings,
    impl_deserialize_from_str, try2,
};

use crate::PREPROCESSOR_NAME;

#[derive(Debug, Default)]
pub struct Config {
    pub repo_url: Option<gix_url::Url>,
    pub site_url: Option<BaseUrl>,
    pub options: Options,
}

impl Config {
    pub fn new(ctx: &PreprocessorContext) -> Result<Self> {
        Self::try_from(ctx.book_toml()).context("invalid config in book.toml")
    }
}

impl TryFrom<BookToml<'_>> for Config {
    type Error = anyhow::Error;

    fn try_from(value: BookToml<'_>) -> Result<Self, Self::Error> {
        try2!({
            let mut book_toml = value.with_source();

            let options = book_toml
                .preprocessor::<Options>(&[PREPROCESSOR_NAME, "mdbook-link-forever"])?
                .unwrap_or_default();

            struct RepoUrl(gix_url::Url);
            impl_deserialize_from_str!(RepoUrl, "a remote URL", |s| {
                Ok(Self(gix_url::parse(s.into())?))
            });

            let repo_url = if options.repo_url_template.template.is_none() {
                book_toml
                    .html_config::<RepoUrl>("git-repository-url")?
                    .map(|u| u.0)
            } else {
                None
            };

            let site_url = book_toml.html_config("site-url")?;

            Ok(Self {
                repo_url,
                site_url,
                options,
            })
        })
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Options {
    #[serde(default, deserialize_with = "value_shorthand::<Url, _, _>")]
    pub repo_url_template: TemplateConfig,
    #[serde(default)]
    pub always_link: Vec<String>,
    #[serde(default)]
    pub remote_name: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "via::<UnstableFeature<ValueShorthand<bool, _>>, _, _>")]
    pub dev_mode: DevMode,
    #[serde(default)]
    pub fail_on_warnings: FailOnWarnings,
    #[serde(default, alias = "book-url")]
    pub site_url: Option<BaseUrl>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TemplateConfig {
    #[serde(default)]
    pub template: Option<Url>,
    #[serde(default)]
    pub params: Option<PathParams>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PathParams {
    #[serde(default, deserialize_with = "value_or_vec1")]
    pub tree: Vec<String>,
    #[serde(default, deserialize_with = "value_or_vec1")]
    pub raw: Vec<String>,
    #[serde(default, deserialize_with = "value_or_vec1")]
    pub commit: Vec<String>,
    #[serde(default, deserialize_with = "value_or_vec1")]
    pub tag: Vec<String>,
}

impl Default for PathParams {
    fn default() -> Self {
        Self {
            tree: vec!["tree".into(), "blob".into()],
            raw: vec!["raw".into()],
            commit: vec!["commit".into()],
            tag: vec!["tag".into()],
        }
    }
}

impl From<Url> for TemplateConfig {
    fn from(value: Url) -> Self {
        Self {
            template: Some(value),
            params: Default::default(),
        }
    }
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(from = "DevModeConfig")]
pub struct DevMode(Option<DevModeConfig>);

impl Deref for DevMode {
    type Target = Option<DevModeConfig>;

    fn deref(&self) -> &Self::Target {
        if is_ci().is_some() { &None } else { &self.0 }
    }
}

impl From<bool> for DevMode {
    fn from(value: bool) -> Self {
        if value {
            Self(Some(Default::default()))
        } else {
            Self(None)
        }
    }
}

impl From<DevModeConfig> for DevMode {
    fn from(value: DevModeConfig) -> Self {
        Self(Some(value))
    }
}

impl From<UnstableFeature<ValueShorthand<bool, Self>>> for DevMode {
    fn from(value: UnstableFeature<ValueShorthand<bool, Self>>) -> Self {
        value.0.0
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DevModeConfig {
    #[serde(default)]
    pub embed_images: Option<bool>,
    #[serde(default = "DevModeConfig::default_editor_uri")]
    pub editor_uri: Url,
}

impl Default for DevModeConfig {
    fn default() -> Self {
        Self {
            embed_images: Some(true),
            editor_uri: Self::default_editor_uri(),
        }
    }
}

impl DevModeConfig {
    fn default_editor_uri() -> Url {
        #[allow(clippy::unwrap_used)]
        "vscode://file/{path}".parse().unwrap()
    }
}

fn value_or_vec1<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let values = value_or_vec(deserializer)?;
    if values.is_empty() {
        let err = serde::de::Error::custom("expected at least 1 item");
        Err(err)
    } else {
        Ok(values)
    }
}
