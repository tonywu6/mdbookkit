use anyhow::{Context, Result};
use mdbook_preprocessor::PreprocessorContext;
use serde::{
    Deserialize, Deserializer,
    de::value::{MapAccessDeserializer, SeqAccessDeserializer},
};
use url::Url;

use mdbookkit::{
    book::PreprocessorHelper,
    config::{BaseUrl, value_or_vec},
    error::{FailOnWarnings, MapDeserializeError},
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
        try2!({
            let mut book_toml = ctx.book_toml().with_source();

            let options = book_toml
                .preprocessor::<Options>(&[PREPROCESSOR_NAME, "mdbook-link-forever"])?
                .unwrap_or_default();

            struct RepoUrl(gix_url::Url);
            impl_deserialize_from_str!(RepoUrl, "a remote URL", |s| {
                Ok(Self(gix_url::parse(s.into())?))
            });

            let repo_url = book_toml
                .html_config::<RepoUrl>("git-repository-url")?
                .map(|u| u.0);

            let site_url = book_toml.html_config::<BaseUrl>("site-url")?;

            Ok(Self {
                repo_url,
                site_url,
                options,
            })
        })
        .context("failed to read config from book.toml")
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Options {
    #[serde(default, deserialize_with = "TemplateConfig::deserialize2")]
    pub repo_url_template: TemplateConfig,
    #[deprecated]
    #[serde(default)]
    pub book_url: Option<BaseUrl>,
    #[serde(default)]
    pub remote_name: Option<String>,
    #[serde(default)]
    pub always_link: Vec<String>,
    #[serde(default)]
    pub fail_on_warnings: FailOnWarnings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TemplateConfig {
    #[serde(default)]
    pub pattern: Option<Url>,
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

impl TemplateConfig {
    fn deserialize2<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        return deserializer.deserialize_any(Visitor);

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TemplateConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a URL or a table")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let pattern = v.parse().or_serde_error()?;
                Ok(TemplateConfig {
                    pattern: Some(pattern),
                    params: Default::default(),
                })
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                Deserialize::deserialize(SeqAccessDeserializer::new(seq))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                Deserialize::deserialize(MapAccessDeserializer::new(map))
            }
        }
    }
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
