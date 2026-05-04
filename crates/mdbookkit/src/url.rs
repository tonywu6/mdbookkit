use std::{
    borrow::Cow, collections::HashMap, fmt::Debug, ops::ControlFlow, path::Path, str::FromStr,
};

use anyhow::{Context, Result};
use tap::Pipe;
use url::{Url, form_urlencoded::Serializer as SearchParams};

#[derive(Clone)]
pub struct UrlPath(Url);

impl UrlPath {
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

    pub fn join(&self, rhs: &str) -> Result<Self, url::ParseError> {
        Ok(Self(self.0.join(rhs)?))
    }

    pub fn relative_to(&self, base: &Self) -> Result<String> {
        let err = || {
            format! { "cannot make a relative URL from {:?} to {:?}",
            base.as_str(), self.as_str() }
        };
        base.0.make_relative(&self.0).with_context(err)
    }

    pub fn as_str(&self) -> &str {
        self.path_only().unwrap_or_else(|| self.0.as_str())
    }

    pub fn as_url(&self) -> Option<&Url> {
        if self.path_only().is_none() {
            Some(&self.0)
        } else {
            None
        }
    }

    pub fn into_url(self) -> Option<Url> {
        if self.as_url().is_some() {
            Some(self.0)
        } else {
            None
        }
    }

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
}

impl UrlUtil for Url {
    fn ensure_trailing_slash(&mut self) {
        if let Ok(mut paths) = self.path_segments_mut() {
            paths.pop_if_empty().push("");
        }
    }
}

impl UrlUtil for UrlPath {
    fn ensure_trailing_slash(&mut self) {
        self.0.ensure_trailing_slash();
    }
}

pub trait ExpectUrl<T> {
    fn expect_url(self) -> T;
}

impl<T> ExpectUrl<T> for Result<T, url::ParseError> {
    #[inline(always)]
    fn expect_url(self) -> T {
        self.expect("should be a valid URL")
    }
}

pub trait UrlFromPath {
    fn to_directory_url(&self) -> Url;

    fn to_file_url(&self) -> Url;
}

impl<P: AsRef<Path> + ?Sized> UrlFromPath for P {
    #[inline(always)]
    fn to_directory_url(&self) -> Url {
        Url::from_directory_path(self).expect("should be a valid absolute path")
    }

    #[inline(always)]
    fn to_file_url(&self) -> Url {
        Url::from_file_path(self).expect("should be a valid absolute path")
    }
}
