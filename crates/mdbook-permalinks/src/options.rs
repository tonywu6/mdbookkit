use std::borrow::Cow;

use serde::{
    Deserialize, Deserializer,
    de::value::{MapAccessDeserializer, SeqAccessDeserializer},
};
use url::Url;

use mdbookkit::{
    config::value_or_vec,
    error::{FailOnWarnings, MapDeserializeError},
    url::UrlUtil,
};

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    #[serde(default, deserialize_with = "TemplateConfig::deserialize2")]
    pub repo_url_template: TemplateConfig,
    #[serde(default)]
    pub book_url: Option<BookUrl>,
    #[serde(default)]
    pub remote_name: Option<String>,
    #[serde(default)]
    pub always_link: Vec<String>,
    #[serde(default)]
    pub fail_on_warnings: FailOnWarnings,
}

#[derive(Debug)]
pub struct BookUrl(Url);

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

impl AsRef<Url> for BookUrl {
    fn as_ref(&self) -> &Url {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BookUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let url = Cow::<str>::deserialize(deserializer)?;
        let url = url.parse::<Url>().or_serde_error()?.with_trailing_slash();
        if url.scheme() != "https" && url.scheme() != "http" {
            let err = serde::de::Error::custom("expected an HTTP URL");
            return Err(err);
        }
        Ok(Self(url))
    }
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
