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
use url::{Url, form_urlencoded};

use crate::error::{Show, WithDebugContext};

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
    ) -> Option<UrlMatch<'a, 'b>>;

    fn as_base<'a>(&'a self) -> BaseUrl<'a>;

    fn with_after_path(self, url: &RelativeUrl) -> Self;
}

#[derive(Debug, Clone, Copy)]
pub struct BaseUrl<'a>(&'a Url);

#[derive(Debug, Clone)]
pub struct RelativeUrl {
    url: String,
    query: Option<usize>,
    fragment: Option<usize>,
}

impl BaseUrl<'_> {
    #[inline]
    pub fn make_relative(self, url: &Url) -> Option<RelativeUrl> {
        #[allow(clippy::disallowed_methods)]
        self.0.make_relative(url).map(RelativeUrl::new)
    }

    #[inline]
    pub fn make_absolute(self, url: &RelativeUrl) -> Url {
        (self.0.join(&url.url)).expect("`url` was created from `make_relative` and should be valid")
    }

    pub fn show_relative(self, url: &Url) -> impl Debug + Display + Show {
        struct ShowUrl<'a, 'b> {
            base: &'a Url,
            url: &'b Url,
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

        impl Show for ShowUrl<'_, '_> {
            fn show(&self) -> impl Debug {
                self
            }
        }

        impl ShowUrl<'_, '_> {
            fn with_string<F, T>(&self, f: F) -> T
            where
                F: FnOnce(Cow<'_, str>) -> T,
            {
                #[allow(clippy::disallowed_methods)]
                if let Some(path) = self.base.make_relative(self.url) {
                    f(percent_decode_str(&path).decode_utf8_lossy())
                } else if let Ok(path) = self.url.to_file_path() {
                    f(path.display().to_string().into())
                } else {
                    f(percent_decode_str(self.url.as_str()).decode_utf8_lossy())
                }
            }
        }

        ShowUrl { base: self.0, url }
    }
}

impl RelativeUrl {
    fn new(url: String) -> Self {
        Self {
            query: url.find('?'),
            fragment: url.find('#'),
            url,
        }
    }

    #[inline]
    pub fn encoded_path(&self) -> &str {
        if let Some(idx) = self.query {
            &self.url[..idx]
        } else if let Some(idx) = self.fragment {
            &self.url[..idx]
        } else {
            &self.url
        }
    }

    #[inline]
    pub fn consume_with<F, T>(self, f: F) -> T
    where
        F: FnOnce(String) -> T,
    {
        f(self.url)
    }

    pub fn query(&self) -> Option<&str> {
        let start = self.query?;
        if let Some(end) = self.fragment {
            Some(&self.url[start + 1..end])
        } else {
            Some(&self.url[start + 1..])
        }
    }

    pub fn fragment(&self) -> Option<&str> {
        Some(&self.url[self.fragment? + 1..])
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
            .fold(
                form_urlencoded::Serializer::new(String::new()),
                |mut search, (k, v)| {
                    if let Some(v) = decode_group(v.as_ref()).and_then(&mut f) {
                        search.append_pair(&k, &v);
                    } else {
                        search.append_pair(&k, &v);
                    }
                    search
                },
            )
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
    ) -> Option<UrlMatch<'a, 'b>> {
        if self.scheme() != value.scheme() || self.authority() != value.authority() {
            return None;
        }

        let mut matches = HashMap::new();

        let mut capture = |lhs: &'a str, rhs: Option<&'b str>| -> ControlFlow<()> {
            match (decode_group(lhs), rhs) {
                (Some(lhs), Some(rhs)) => {
                    matches.insert(Cow::Borrowed(lhs), Cow::Borrowed(rhs));
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
            matches.insert(Cow::Borrowed(group), Cow::Owned(all));
        }

        let mut query_patterns = self.query_pairs().collect::<HashMap<_, _>>();
        let query = if !query_patterns.is_empty() {
            let mut query = (value.query().map(|q| q.len()).unwrap_or(0))
                .pipe(String::with_capacity)
                .pipe(form_urlencoded::Serializer::new);

            for (name, value) in value.query_pairs() {
                if let Some(group) = query_patterns.remove(&*name) {
                    // both urls have this param
                    if let Some(group) = decode_group(&group) {
                        matches.insert(group.to_owned().into(), value);
                    } else if value != group {
                        // pattern specifies a constant value for this param in which case
                        // the url being tested must also have the same value
                        return None;
                    }
                } else {
                    // only the url being tested has this param
                    query.append_pair(&name, &value);
                }
            }

            if !query_patterns.is_empty() {
                // pattern has required params that the url being tested doesn't have
                return None;
            }

            Some(query.finish().into())
        } else {
            value.query().map(<_>::into)
        };

        let fragment = if let Some(lhs) = self.fragment() {
            if let Some(grp) = decode_group(lhs) {
                if let Some(rhs) = value.fragment() {
                    matches.insert(grp.into(), rhs.into());
                    None
                } else {
                    return None;
                }
            } else if let Some(rhs) = value.fragment() {
                if lhs == rhs {
                    None
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            value.fragment()
        };

        Some(UrlMatch {
            matches,
            query,
            fragment,
        })
    }

    #[inline]
    fn as_base<'a>(&'a self) -> BaseUrl<'a> {
        BaseUrl(self)
    }

    #[inline]
    fn with_after_path(mut self, url: &RelativeUrl) -> Self {
        match (self.query(), url.query()) {
            (_, None) => {}
            (None, Some(query)) => self.set_query(Some(query)),
            (Some(..), Some(query)) => {
                self.query_pairs_mut()
                    .extend_pairs(form_urlencoded::parse(query.as_bytes()))
                    .finish();
            }
        }
        if let Some(fragment) = url.fragment() {
            self.set_fragment(Some(fragment));
        }
        self
    }
}

#[derive(Debug)]
pub struct UrlMatch<'pat, 'url> {
    pub matches: HashMap<Cow<'pat, str>, Cow<'url, str>>,
    pub query: Option<Cow<'url, str>>,
    pub fragment: Option<&'url str>,
}

impl UrlMatch<'_, '_> {
    pub fn to_relative_url(&self, path: &str) -> Option<RelativeUrl> {
        let mut url = match self.matches.get(path)? {
            Cow::Borrowed(url) => (*url).to_owned(),
            Cow::Owned(url) => url.clone(),
        };

        let query = if let Some(ref query) = self.query {
            let idx = url.len();
            url.push('?');
            url.push_str(query);
            Some(idx)
        } else {
            None
        };

        let fragment = if let Some(fragment) = self.fragment {
            let idx = url.len();
            url.push('#');
            url.push_str(fragment);
            Some(idx)
        } else {
            None
        };

        Some(RelativeUrl {
            url,
            query,
            fragment,
        })
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

pub trait UrlFromPath {
    fn dir_to_url(&self) -> Url;
    fn file_to_url(&self) -> Url;
}

impl<P: AsRef<Path> + ?Sized> UrlFromPath for P {
    #[inline]
    fn dir_to_url(&self) -> Url {
        let path = self.as_ref();
        #[allow(clippy::disallowed_methods)]
        Url::from_directory_path(path).expect("should be a valid absolute path")
    }

    #[inline]
    fn file_to_url(&self) -> Url {
        let path = self.as_ref();
        #[allow(clippy::disallowed_methods)]
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
        #[allow(clippy::disallowed_methods)]
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
        #[allow(clippy::disallowed_methods)]
        Utf8Path::from_path(self.as_path())
            .with_path_debug(self)
            .context(UTF8_PATH_ERROR)
    }

    #[inline]
    fn into_utf8_path(self) -> Result<Utf8PathBuf> {
        #[allow(clippy::disallowed_methods)]
        match Utf8PathBuf::from_path_buf(self) {
            Ok(path) => Ok(path),
            Err(bad) => Err(anyhow!("{:?}", bad.display())).context(UTF8_PATH_ERROR),
        }
    }
}

static UTF8_PATH_ERROR: &str = "path contains non-UTF-8 characters, which is unsupported";
