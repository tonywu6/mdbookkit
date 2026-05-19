use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::Debug,
    ops::ControlFlow,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use tap::Pipe;
use url::{Url, form_urlencoded::Serializer as SearchParams};

use crate::error::WithPathDebug;

#[derive(Clone)]
pub struct UrlPath(Url);

impl UrlPath {
    #[inline]
    pub fn fill_pattern<'a, F>(&self, mut f: F) -> Self
    where
        F: for<'b> FnMut(&'b str) -> Option<Cow<'a, str>>,
    {
        let path = (self.0.path().split('/'))
            .map(|segment| {
                decode_group(segment)
                    .and_then(&mut f)
                    .unwrap_or(Cow::Borrowed(segment))
            })
            .collect::<Vec<_>>()
            .join("/");

        let query = (self.0.query_pairs())
            .fold(SearchParams::new(String::new()), |mut search, (k, v)| {
                if let Some(v) = decode_group(v.as_ref()).and_then(&mut f) {
                    search.append_pair(&k, &v);
                } else {
                    search.append_pair(&k, &v);
                }
                search
            })
            .finish()
            .pipe(|query| if query.is_empty() { None } else { Some(query) });

        let mut url = self.0.clone();
        url.set_path(&path);
        url.set_query(query.as_deref());
        Self(url)
    }

    #[inline]
    pub fn test_pattern<'a, 'b>(
        &'a self,
        catch_all: Option<&'a str>,
        value: &'b Url,
    ) -> Option<HashMap<Cow<'a, str>, Cow<'b, str>>> {
        if self.path_only().is_none()
            && (self.0.scheme() != value.scheme() || self.0.authority() != value.authority())
        {
            return None;
        }

        let mut captured = HashMap::new();

        let mut capture = |lhs: &'a str, rhs: Option<&'b str>| -> ControlFlow<()> {
            match (decode_group(lhs), rhs) {
                (Some(lhs), Some(rhs)) => {
                    captured.insert(Cow::Borrowed(lhs), Cow::Borrowed(rhs));
                    ControlFlow::Continue(())
                }
                (None, Some(rhs)) if lhs == rhs => ControlFlow::Continue(()),
                _ => ControlFlow::Break(()),
            }
        };

        let mut lhs = self.0.path().split('/');
        let mut rhs = value.path().split('/');

        #[allow(clippy::while_let_on_iterator, reason = "symmetry")]
        while let Some(lhs) = lhs.next() {
            if decode_group(lhs) == catch_all {
                break;
            }
            match capture(lhs, rhs.next()) {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return None,
            }
        }

        while let Some(lhs) = lhs.next_back() {
            match capture(lhs, rhs.next_back()) {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(()) => return None,
            }
        }

        if let Some(group) = catch_all {
            // nightly: https://doc.rust-lang.org/stable/core/str/struct.Split.html#method.remainder
            let all = rhs.collect::<Vec<_>>().join("/");
            captured.insert(Cow::Borrowed(group), Cow::Owned(all));
        }

        let mut rhs = value.query_pairs().collect::<HashMap<_, _>>();

        for (k, v) in self.0.query_pairs() {
            match (decode_group(v.as_ref()), rhs.remove(&k)) {
                (Some(lhs), Some(rhs)) => {
                    captured.insert(lhs.to_owned().into(), rhs);
                }
                (None, Some(rhs)) if v == rhs => {}
                _ => return None,
            }
        }

        Some(captured)
    }

    #[inline]
    pub fn join(&self, rhs: &str) -> Result<Self, url::ParseError> {
        Ok(Self(self.0.join(rhs)?))
    }

    #[inline]
    pub fn relative_to(&self, base: &Self) -> Result<String> {
        let err = || {
            format! { "cannot make a relative URL from {:?} to {:?}",
            base.as_str(), self.as_str() }
        };
        base.0.make_relative(&self.0).with_context(err)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.path_only().unwrap_or_else(|| self.0.as_str())
    }

    #[inline]
    pub fn is_url(&self) -> bool {
        self.path_only().is_none()
    }

    #[inline]
    pub fn into_url(self) -> Option<Url> {
        if self.is_url() { Some(self.0) } else { None }
    }

    #[inline]
    pub fn empty() -> Self {
        "".parse().expect_url()
    }

    fn path_only(&self) -> Option<&str> {
        self.0.as_str().strip_prefix("example:")
    }
}

impl FromStr for UrlPath {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        "example:/".parse::<Url>()?.join(s).map(Self)
    }
}

impl From<Url> for UrlPath {
    fn from(value: Url) -> Self {
        Self(value)
    }
}

impl Debug for UrlPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UrlPattern")
            .field(&format_args!("{:?}", self.as_str()))
            .finish()
    }
}

/// - `{` and `}` may be percent-encoded in path [^1].
/// - Encoding characters are always in uppercase [^2].
///
/// [^1]: <https://url.spec.whatwg.org/#path-percent-encode-set>
/// [^2]: <https://url.spec.whatwg.org/#percent-encode>
fn decode_group(segment: &str) -> Option<&str> {
    if segment.strip_prefix("%7B%7B").is_some() || segment.strip_prefix("{{").is_some() {
        None
    } else if let Some(segment) =
        (segment.strip_prefix("%7B")).or_else(|| segment.strip_prefix('{'))
    {
        (segment.strip_suffix("%7D")).or_else(|| segment.strip_suffix('}'))
    } else {
        None
    }
}

pub trait UrlUtil {
    fn ensure_trailing_slash(&mut self);

    #[inline]
    fn with_trailing_slash(mut self) -> Self
    where
        Self: Sized,
    {
        self.ensure_trailing_slash();
        self
    }
}

impl UrlUtil for Url {
    #[inline]
    fn ensure_trailing_slash(&mut self) {
        if let Ok(mut paths) = self.path_segments_mut() {
            paths.pop_if_empty().push("");
        }
    }
}

impl UrlUtil for UrlPath {
    #[inline]
    fn ensure_trailing_slash(&mut self) {
        self.0.ensure_trailing_slash();
    }
}

pub trait ExpectUrl<T> {
    fn expect_url(self) -> T;
}

impl<T> ExpectUrl<T> for Result<T, url::ParseError> {
    #[inline]
    fn expect_url(self) -> T {
        self.expect("should be a valid URL")
    }
}

pub trait ExpectPath {
    fn expect_path(&self) -> Utf8PathBuf;
}

impl ExpectPath for Url {
    #[inline]
    fn expect_path(&self) -> Utf8PathBuf {
        self.to_file_path()
            .expect("should be a file: url")
            .into_utf8_path_buf()
            .expect("url is already in utf-8")
    }
}

pub trait UrlFromPath {
    fn to_directory_url(&self) -> Url;

    fn to_file_url(&self) -> Url;
}

impl<P: AsRef<Path> + ?Sized> UrlFromPath for P {
    #[inline]
    fn to_directory_url(&self) -> Url {
        Url::from_directory_path(self).expect("should be a valid absolute path")
    }

    #[inline]
    fn to_file_url(&self) -> Url {
        Url::from_file_path(self).expect("should be a valid absolute path")
    }
}

pub trait ToUtf8Path {
    fn to_utf8_path(&self) -> Result<&Utf8Path>;
    fn into_utf8_path_buf(self) -> Result<Utf8PathBuf>;
}

impl ToUtf8Path for &Path {
    #[inline]
    fn to_utf8_path(&self) -> Result<&Utf8Path> {
        Utf8Path::from_path(self)
            .with_path_debug(self)
            .context(UTF8_PATH_ERROR)
    }

    #[inline]
    fn into_utf8_path_buf(self) -> Result<Utf8PathBuf> {
        Ok(self.to_utf8_path()?.to_owned())
    }
}

impl ToUtf8Path for PathBuf {
    #[inline]
    fn to_utf8_path(&self) -> Result<&Utf8Path> {
        Utf8Path::from_path(self.as_path())
            .with_path_debug(self)
            .context(UTF8_PATH_ERROR)
    }

    #[inline]
    fn into_utf8_path_buf(self) -> Result<Utf8PathBuf> {
        match Utf8PathBuf::from_path_buf(self) {
            Ok(path) => Ok(path),
            Err(bad) => Err(anyhow!("{:?}", bad.display())).context(UTF8_PATH_ERROR),
        }
    }
}

static UTF8_PATH_ERROR: &str = "path contains non-UTF-8 characters, which is unsupported";
