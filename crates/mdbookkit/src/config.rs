use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt::Debug,
    marker::PhantomData,
    path::{self, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use camino::{Utf8Component, Utf8Path};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::value::{EnumAccessDeserializer, MapAccessDeserializer, SeqAccessDeserializer},
};
use url::Url;

use crate::{
    book::{BookToml, string_from_stdin},
    error::{MapDeserializeError, Show},
    markdown::Spanned,
    url::{UrlFromPath, UrlUtil},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BaseUrl(BaseUrlValue);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum BaseUrlValue {
    Http(Url),
    Path {
        path: PathBuf,
        search: Option<String>,
        hash: Option<String>,
    },
}

impl BaseUrl {
    pub fn resolve(self, root: PathBuf) -> (Url, Option<PathBuf>) {
        match self.0 {
            BaseUrlValue::Http(http) => (http, None),
            BaseUrlValue::Path { path, search, hash } => {
                let path = root.join(&path);
                let mut url = path.dir_to_url();
                url.set_query(search.as_deref());
                url.set_fragment(hash.as_deref());
                (url, Some(path))
            }
        }
    }
}

impl FromStr for BaseUrl {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl FromStr for BaseUrlValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<Url>() {
            Ok(url) => {
                if !matches!(url.scheme(), "https" | "http") {
                    bail!("expected an HTTP URL")
                }
                let url = url.with_trailing_slash();
                Ok(Self::Http(url))
            }

            Err(..) => {
                for part in Utf8Path::new(s).components() {
                    match part {
                        Utf8Component::Prefix(p) => {
                            bail!("a base URL cannot contain `{p}`")
                        }
                        Utf8Component::ParentDir => {
                            bail!("a base URL cannot contain `{part}`")
                        }
                        Utf8Component::RootDir => {}
                        Utf8Component::CurDir => {}
                        Utf8Component::Normal(..) => {}
                    }
                }

                let url = if cfg!(windows) {
                    Utf8Path::new("C:\\").dir_to_url()
                } else {
                    #[allow(clippy::unwrap_used)]
                    "file:///".parse::<Url>().unwrap()
                };
                let url = (url.join(s)).context("this path results in an invalid base URL")?;

                let path = match url.to_file_path() {
                    Err(()) => bail!("this path is invalid on this system"),
                    Ok(path) => path
                        .components()
                        .fold(
                            PathBuf::with_capacity(path.capacity()),
                            |base, part| match part {
                                #[cfg(windows)]
                                path::Component::Prefix(..) => base,
                                #[cfg(not(windows))]
                                path::Component::Prefix(..) => unreachable!(),
                                path::Component::ParentDir => unreachable!(),
                                path::Component::RootDir => base,
                                path::Component::CurDir => base,
                                path::Component::Normal(part) => base.join(part),
                            },
                        )
                        .join(""),
                };

                let search = if let search =
                    &url[url::Position::AfterPath..url::Position::AfterQuery]
                    && !search.is_empty()
                {
                    Some(search[1..].to_owned())
                } else {
                    None
                };
                let hash = if let hash = &url[url::Position::AfterQuery..]
                    && !hash.is_empty()
                {
                    Some(hash[1..].to_owned())
                } else {
                    None
                };

                Ok(Self::Path { path, search, hash })
            }
        }
    }
}

impl<'de> Deserialize<'de> for BaseUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let url = Cow::<str>::deserialize(deserializer)?;
        let url = url.parse().or_serde_error()?;
        Ok(url)
    }
}

impl Show for BaseUrl {
    fn show(&self) -> impl Debug {
        self.0.show()
    }
}

impl Show for BaseUrlValue {
    fn show(&self) -> impl Debug {
        struct ShowBaseUrl<'a>(&'a BaseUrlValue);
        return ShowBaseUrl(self);
        impl Debug for ShowBaseUrl<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    BaseUrlValue::Http(http) => http.show().fmt(f),
                    BaseUrlValue::Path { path, search, hash } => {
                        let mut text = format!("{}", path.display());
                        if let Some(search) = search {
                            text.push('?');
                            text.push_str(search);
                        }
                        if let Some(hash) = hash {
                            text.push('#');
                            text.push_str(hash);
                        }
                        text.show().fmt(f)
                    }
                }
            }
        }
    }
}

#[inline]
pub fn value_or_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(PhantomData<T>);

    macro_rules! forward {
        ($f:ident($v:ty)) => {
            fn $f<E>(self, v: $v) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                use serde::de::IntoDeserializer;
                Ok(vec![T::deserialize(v.into_deserializer())?])
            }
        };
    }

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an item or a list of items")
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            Vec::deserialize(SeqAccessDeserializer::new(seq))
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            Ok(vec![T::deserialize(MapAccessDeserializer::new(map))?])
        }

        fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::EnumAccess<'de>,
        {
            Ok(vec![T::deserialize(EnumAccessDeserializer::new(data))?])
        }

        forward!(visit_bool(bool));
        forward!(visit_i8(i8));
        forward!(visit_i16(i16));
        forward!(visit_i32(i32));
        forward!(visit_i64(i64));
        forward!(visit_i128(i128));
        forward!(visit_u8(u8));
        forward!(visit_u16(u16));
        forward!(visit_u32(u32));
        forward!(visit_u64(u64));
        forward!(visit_u128(u128));
        forward!(visit_f32(f32));
        forward!(visit_f64(f64));
        forward!(visit_char(char));
        forward!(visit_str(&str));
        forward!(visit_borrowed_str(&'de str));
        forward!(visit_string(String));
        forward!(visit_bytes(&[u8]));
        forward!(visit_borrowed_bytes(&'de [u8]));
        forward!(visit_byte_buf(Vec<u8>));
    }

    deserializer.deserialize_any(Visitor(PhantomData))
}

#[macro_export]
macro_rules! de_struct {
    (@derive $(#[$struct_att_:meta])* [$(($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)))*] []) => {$(
        de_struct!(@deserialize $(#[$struct_attr])* $name ($($body)*));
    )*};
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)*] [$($rest)*]);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)*] []);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*, $($rest)*]);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*]);
    };

    (@deserialize $(#[$struct_attr:meta])* $name:ident ($($body:tt)*)) => {
        #[automatically_derived]
        #[allow(non_camel_case_types)]
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                de_struct!(@define $(#[$struct_attr])* $name [] [] [$($body)*]);
                let de_struct!(@unpack $name [] [$($body)*]) = ::serde::Deserialize::deserialize(deserializer)?;
                #[allow(clippy::redundant_field_names)]
                Ok(de_struct!(@result Self [] [$($body)*]))
            }
        }
    };

    (@define $(#[$struct_attr:meta])* $name:ident [$(($(#[$field_attr:meta])* $field:ident $type:ty))*] [$($infer:ident)*] []) => {
        #[derive(::serde::Deserialize)]
        $(#[$struct_attr])*
        struct $name<$($infer),*> {
            $($(#[$field_attr])* $field: $type),*
        }
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident, $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] [$($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] []);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty, $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] [$($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] []);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*, $($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*]);
    };

    (@unpack $name:ident [$($field:ident)*] []) => {
        $name { $($field),* }
    };
    (@unpack $name:ident [$($field:ident)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@unpack $name [$($field)* $next] [$($rest)*])
    };
    (@unpack $name:ident [$($field:ident)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?]) => {
        de_struct!(@unpack $name [$($field)* $next] [])
    };
    (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@unpack $name [$($field)*] [$($body)*, $($rest)*])
    };
    (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@unpack $name [$($field)*] [$($body)*])
    };

    (@result $name:ident [$(($field:ident: $($value:tt)*))*] []) => {
        $name {
            $($field: $($value)*),*
        }
    };
    (@result $name:ident [$($item:tt)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@result $name [$($item)* ($next: $next)] [$($rest)*])
    };
    (@result $name:ident [$($item:tt)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?]) => {
        de_struct!(@result $name [$($item)* ($next: $next)] [])
    };
    (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [$($rest)*])
    };
    (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [])
    };

    ($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)) => {
        de_struct!(@derive $(#[$struct_attr])* [($(#[$struct_attr])* $name ($($body)*))] [$($body)*]);
    };
}

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct ConfigExampleInputs(pub BTreeMap<String, Vec<Spanned<String>>>);

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct ConfigExampleErrors(pub BTreeMap<String, Vec<Spanned<String>>>);

pub fn validate_config_examples<T>(preprocessor: &str) -> Result<()>
where
    T: for<'de> Deserialize<'de>,
{
    let input = string_from_stdin()?;
    let input = serde_json::from_str::<ConfigExampleInputs>(&input)?;
    let errors = (input.0.into_iter())
        .filter_map(|(name, examples)| {
            let errors = examples
                .into_iter()
                .filter_map(|(example, span)| {
                    match (example.parse::<BookToml>())
                        .and_then(|mut config| config.preprocessor::<T>(&[preprocessor]))
                        .and_then(|config| config.context("config table not defined"))
                    {
                        Ok(..) => None,
                        Err(e) => Some((format!("{e:?}"), span)),
                    }
                })
                .collect::<Vec<_>>();
            if errors.is_empty() {
                None
            } else {
                Some((name, errors))
            }
        })
        .collect();
    let errors = ConfigExampleErrors(errors);
    if errors.0.is_empty() {
        Ok(())
    } else {
        let errors = serde_json::to_string(&errors)?;
        println!("{errors}");
        bail!("Some config snippets failed to validate")
    }
}
