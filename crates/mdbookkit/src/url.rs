use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{Debug, Display},
    ops::ControlFlow,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use percent_encoding::percent_decode_str;
use tap::Pipe;
use url::{Url, form_urlencoded::Serializer as SearchParams};

use crate::error::WithPathDebug;

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

    fn pattern_fill<'a, F>(&self, f: F) -> Url
    where
        F: for<'b> FnMut(&'b str) -> Option<Cow<'a, str>>;

    fn pattern_test<'a, 'b>(
        &'a self,
        spread: Option<&'a str>,
        value: &'b Url,
    ) -> Option<HashMap<Cow<'a, str>, Cow<'b, str>>>;

    fn remove_suffix(&self, from: Url) -> (Url, UrlSuffix);

    fn expect_path(&self) -> PathBuf;

    fn debug(&self) -> impl Debug;

    fn print_relative(&self, path: &Url) -> impl Debug + Display;
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

#[must_use]
#[derive(Debug)]
pub struct UrlSuffix {
    query: HashMap<String, String>,
    fragment: Option<String>,
}

impl UrlSuffix {
    pub fn restored(self, mut url: Url) -> Url {
        let Self { query, fragment } = self;

        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query).finish();
        }

        match (url.fragment(), &fragment) {
            (Some(_), None) => {}
            _ => url.set_fragment(fragment.as_deref()),
        }

        url
    }
}

impl UrlUtil for Url {
    #[inline]
    fn ensure_trailing_slash(&mut self) {
        if let Ok(mut paths) = self.path_segments_mut() {
            paths.pop_if_empty().push("");
        } else {
            let path = self.path();
            if !path.ends_with('/') {
                self.set_path(&format!("{path}/"));
            }
        }
    }

    fn pattern_fill<'a, F>(&self, mut f: F) -> Url
    where
        F: for<'b> FnMut(&'b str) -> Option<Cow<'a, str>>,
    {
        let path = (self.path().split('/'))
            .map(|segment| {
                decode_group(segment)
                    .and_then(&mut f)
                    .unwrap_or(Cow::Borrowed(segment))
            })
            .collect::<Vec<_>>()
            .join("/");

        let query = (self.query_pairs())
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

        let fragment = self.fragment().and_then(decode_group).and_then(&mut f);

        let mut url = self.clone();

        url.set_path(&path);
        url.set_query(query.as_deref());

        if let Some(f) = fragment {
            url.set_fragment(Some(&*f));
        }

        url
    }

    fn pattern_test<'a, 'b>(
        &'a self,
        spread: Option<&'a str>,
        value: &'b Url,
    ) -> Option<HashMap<Cow<'a, str>, Cow<'b, str>>> {
        if self.scheme() != value.scheme() || self.authority() != value.authority() {
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

        let mut lhs = self.path().split('/');
        let mut rhs = value.path().split('/');

        #[allow(clippy::while_let_on_iterator, reason = "symmetry")]
        while let Some(lhs) = lhs.next() {
            if decode_group(lhs) == spread {
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

        if let Some(group) = spread {
            // nightly: https://doc.rust-lang.org/stable/core/str/struct.Split.html#method.remainder
            let all = rhs.collect::<Vec<_>>().join("/");
            captured.insert(Cow::Borrowed(group), Cow::Owned(all));
        }

        let mut rhs = value.query_pairs().collect::<HashMap<_, _>>();

        for (k, v) in self.query_pairs() {
            match (decode_group(v.as_ref()), rhs.remove(&k)) {
                (Some(lhs), Some(rhs)) => {
                    captured.insert(lhs.to_owned().into(), rhs);
                }
                (None, Some(rhs)) if v == rhs => {}
                _ => return None,
            }
        }

        if let Some(k) = self.fragment().and_then(decode_group)
            && let Some(v) = value.fragment()
        {
            captured.insert(k.into(), v.into());
        }

        Some(captured)
    }

    fn remove_suffix(&self, mut from: Url) -> (Url, UrlSuffix) {
        let mut query = from.query_pairs().into_owned().collect::<HashMap<_, _>>();

        for (k, v) in self.query_pairs() {
            if decode_group(v.as_ref()).is_some() {
                query.remove(&*k);
            }
        }

        let fragment = if self.fragment().and_then(decode_group).is_some() {
            None
        } else {
            from.fragment().map(<_>::to_owned)
        };

        let suffix = UrlSuffix { query, fragment };

        from.set_query(None);
        from.set_fragment(None);

        (from, suffix)
    }

    #[inline]
    fn expect_path(&self) -> PathBuf {
        self.to_file_path()
            .expect("should have been a valid `file:` url")
    }

    #[inline]
    fn debug(&self) -> impl Debug {
        struct UrlDebug<'a>(&'a Url);
        return UrlDebug(self);
        impl Debug for UrlDebug<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self.0.as_str())
            }
        }
    }

    fn print_relative(&self, path: &Url) -> impl Debug + Display {
        struct ShowUrl<'a, 'b> {
            base: &'a Url,
            path: &'b Url,
        }

        impl Debug for ShowUrl<'_, '_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.with_string(|s| write!(f, "{s:?}"))
            }
        }

        impl Display for ShowUrl<'_, '_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.with_string(|s| write!(f, "{s}"))
            }
        }

        impl ShowUrl<'_, '_> {
            fn with_string<F, T>(&self, f: F) -> T
            where
                F: FnOnce(Cow<'_, str>) -> T,
            {
                if let Some(path) = self.base.make_relative(self.path) {
                    f(percent_decode_str(&path).decode_utf8_lossy())
                } else if let Ok(path) = self.path.to_file_path() {
                    f(path.display().to_string().into())
                } else {
                    f(percent_decode_str(self.path.as_str()).decode_utf8_lossy())
                }
            }
        }

        ShowUrl { base: self, path }
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

pub trait UrlFromPath {
    fn dir_to_url(&self) -> Url;
    fn file_to_url(&self) -> Url;
}

impl<P: AsRef<Path> + ?Sized> UrlFromPath for P {
    #[inline]
    fn dir_to_url(&self) -> Url {
        let path = self.as_ref();
        Url::from_directory_path(path).expect("should be a valid absolute path")
    }

    #[inline]
    fn file_to_url(&self) -> Url {
        let path = self.as_ref();
        Url::from_file_path(path).expect("should be a valid absolute path")
    }
}

pub trait ToUtf8Path {
    fn to_utf8_path(&self) -> Result<&Utf8Path>;
    fn into_utf8_path(self) -> Result<Utf8PathBuf>;
}

impl ToUtf8Path for &Path {
    #[inline]
    fn to_utf8_path(&self) -> Result<&Utf8Path> {
        Utf8Path::from_path(self)
            .with_path_debug(self)
            .context(UTF8_PATH_ERROR)
    }

    #[inline]
    fn into_utf8_path(self) -> Result<Utf8PathBuf> {
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
    fn into_utf8_path(self) -> Result<Utf8PathBuf> {
        match Utf8PathBuf::from_path_buf(self) {
            Ok(path) => Ok(path),
            Err(bad) => Err(anyhow!("{:?}", bad.display())).context(UTF8_PATH_ERROR),
        }
    }
}

static UTF8_PATH_ERROR: &str = "path contains non-UTF-8 characters, which is unsupported";
