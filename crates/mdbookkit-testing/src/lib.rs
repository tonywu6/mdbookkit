use std::{borrow::Cow, ffi::OsStr, fmt::Display, path::Path, sync::LazyLock};

use anstyle::RgbColor;
use anstyle_svg::Palette;
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;
use snapbox::{
    Assert, Data, IntoData, RedactedValue, Redactions, assert::DEFAULT_ACTION_ENV, cmd::Command,
    data::DataFormat, dir::DirRoot,
};
use tap::TryConv;

pub use anyhow;
pub use regex;
pub use snapbox;

pub struct TestBook {
    pub path: TestRoot<'static>,
    pub code: i32,
    pub env_vars: Vec<(&'static str, &'static str)>,
    pub redacted: Vec<(&'static str, RedactedValue)>,
}

impl TestBook {
    pub fn run(&self) -> Result<()> {
        let temp_dir = DirRoot::mutable_temp()?;
        let temp_dir = temp_dir
            .path()
            .expect("temp dir should have a path")
            .try_conv::<&Utf8Path>()?;

        self.cargo("clean", self.path.manifest_dir())
            .assert()
            .success();

        let result = self
            .cargo("bin", &self.path.root_dir)
            .current_dir(&self.path.root_dir)
            .args(["mdbook", "build"])
            .arg(self.path.book_dir())
            .env("MDBOOK_build__build_dir", temp_dir)
            .envs(load_env(&[
                ("MDBOOK_LOG", "warn,mdbookkit::diagnostics=info"),
                ("MDBOOKKIT_TERM_GRAPHICAL", "ascii"),
                ("FORCE_COLOR", "1"),
                ("RUST_BACKTRACE", "0"),
                ("CI", ""),
            ]))
            .envs(load_env(&self.env_vars))
            .assert()
            .code(self.code);

        let stderr_txt = self.path.stderr_txt();
        let stderr_svg = self.path.stderr_svg();

        let stderr = &*result.get_output().stderr;
        let stderr = String::from_utf8_lossy(stderr);

        eprint!("--- stderr\n{stderr}");

        let assert = self.assert()?;

        let mut results = vec![
            assert.try_eq_text(None, &stderr, stderr_svg),
            assert.try_eq_text(None, &stderr, stderr_txt),
        ];

        if self.code == 0 {
            for page in self.path.expected_pages()? {
                let page = page?;
                let name = page.name();
                let expected = page.expected();
                let actual = self.path.actual_page(name, temp_dir)?;
                results.push(assert.try_eq_text(Some(&name), actual, expected));
            }
        }

        for result in results.iter() {
            if let Err(error) = result {
                eprintln!("{error}")
            }
        }

        if results.iter().any(Result::is_err) {
            panic!("some snapshots have changed")
        }

        Ok(())
    }

    pub fn assert(&self) -> Result<Assert> {
        let mut redactions = Redactions::new();
        redactions.insert("[TEST_DIR]", self.path.root_dir.as_str().to_owned())?;
        redactions.insert("[ELAPSED]", Regex::new(r"in (?<redacted>\d+\.\d+s)")?)?;
        redactions.insert(
            "[LLVM_COV_STDERR]",
            Regex::new(r"(?<redacted>error: process didn't exit successfully:.*)")?,
        )?;
        for (placeholder, matcher) in &self.redacted {
            redactions.insert(placeholder, matcher.clone())?;
        }
        Ok(default_assert().redact_with(redactions))
    }

    pub fn cargo(&self, command: &str, wd: impl AsRef<Path>) -> Command {
        Command::new(env!("CARGO")).arg(command).current_dir(wd)
    }
}

#[macro_export]
macro_rules! test_mdbook {
    [
        @init $name:ident exit($code:literal)
        $( , env = [$($env:tt)*] )?
        $( , redacted = [$($redacted:tt)*] )?
        $( , manifest = $manifest:literal )?
    ] => {
        $crate::TestBook {
            path: $crate::TestRoot {
                name: stringify!($name),
                root_dir: $crate::snapbox::current_dir!().try_into()?,
                rust_dir: {
                    let dir = ".";
                    $(let dir = $manifest;)?
                    dir
                },
            },
            code: $code,
            env_vars: $crate::test_mdbook!(@key_values $($($env)*)?),
            redacted: $crate::test_mdbook!(@key_values $($($redacted)*)?),
        }
    };

    [$name:ident $(($shared:ident))?, $($args:tt)+] => {
        #[test]
        fn $name() -> $crate::anyhow::Result<()> {
            // must init struct within test to have
            // "update snapshots" editor action
            $crate::test_mdbook!(@init $name $($args)+).run()
        }

        $crate::test_mdbook!(@newtype $name ($($shared)?) ($($args)+));
    };

    (@newtype $name:ident ($shared:ident) ($($args:tt)+)) => {
        pub struct $shared($crate::TestBook);

        impl $shared {
            pub fn book() -> $crate::anyhow::Result<$crate::TestBook> {
                Ok($crate::test_mdbook!(@init $name $($args)+))
            }
        }
    };
    (@newtype $name:ident () ($($args:tt)+)) => {};

    (@key_values $($key:literal = $val:expr),*) => {
        vec![$(($key, $val)),*]
    };
    (@key_values $($tt:tt)+) => {
        $($tt)+
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct TestRoot<'a> {
    pub root_dir: Utf8PathBuf,
    pub rust_dir: &'a str,
    pub name: &'a str,
}

impl TestRoot<'_> {
    pub fn book_dir(&self) -> Utf8PathBuf {
        self.root_dir.join(self.name)
    }

    pub fn dist_dir(&self) -> Utf8PathBuf {
        self.book_dir().join("out")
    }

    fn manifest_dir(&self) -> Utf8PathBuf {
        self.book_dir().join(self.rust_dir)
    }

    fn stderr_txt(&self) -> Data {
        self.test_data("stderr/data.txt", DataFormat::Text)
    }

    fn stderr_svg(&self) -> Data {
        self.test_data("stderr/data.svg", DataFormat::TermSvg)
    }

    pub fn expected_pages(&self) -> Result<impl Iterator<Item = Result<TestPage<'_>>>> {
        let root = self.book_dir();

        let iter = std::fs::read_dir(root.join("src").as_std_path())
            .context("error reading src dir")?
            .map(|path| {
                let path = path?.path().try_conv::<Utf8PathBuf>()?;

                if path.extension() != Some("md") || path.file_name() == Some("SUMMARY.md") {
                    return Ok(None);
                }

                let root = self.book_dir();
                let name = path
                    .strip_prefix(root.join("src"))
                    .expect("path is under src dir")
                    .to_owned();

                Ok(Some(TestPage { name, root: self }))
            })
            .filter_map(Result::transpose);

        Ok(iter)
    }

    fn actual_page(&self, name: &Utf8Path, temp: &Utf8Path) -> Result<String> {
        let text = std::fs::read_to_string(temp.join(name))
            .with_context(|| name.to_string())
            .context("mdbook did not build this file")?;
        Ok(text)
    }

    fn test_data(&self, path: impl AsRef<Path>, format: DataFormat) -> Data {
        Data::read_from(&self.book_dir().join_os(path), Some(format))
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct TestPage<'a> {
    name: Utf8PathBuf,
    root: &'a TestRoot<'a>,
}

impl TestPage<'_> {
    pub fn name(&self) -> &Utf8Path {
        &self.name
    }

    pub fn expected(&self) -> Data {
        let path = Utf8Path::new("out").join(&self.name);
        self.root.test_data(path, DataFormat::Text)
    }

    pub fn toc_item(&self) -> String {
        format!("- []({})", self.name)
    }

    pub fn mod_item(&self) -> String {
        let path = &self.name;
        let name = self.mod_name();
        format!("#[doc = include_str!({path:?})]\npub mod {name} {{}}")
    }

    pub fn mod_name(&self) -> String {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[^a-z_]+|[^a-z0-9_]+").unwrap());
        RE.replace_all(self.name.with_extension("").as_str(), "_")
            .into()
    }
}

fn load_env<'a>(vars: &[(&'a str, &str)]) -> impl Iterator<Item = (&'a str, impl AsRef<OsStr>)> {
    vars.iter().map(|(key, default)| {
        let val = if let Some(overridden) = std::env::var_os(key) {
            eprintln!(
                "--- overriding env var {key:?} = {:?} (over {default:?})",
                &*overridden.to_string_lossy()
            );
            Cow::Owned(overridden)
        } else {
            Cow::Borrowed(default.as_ref())
        };
        (*key, val)
    })
}

pub trait AssertUtil {
    fn try_eq_text(
        &self,
        name: Option<&dyn Display>,
        actual: impl AsRef<str>,
        expected: Data,
    ) -> snapbox::assert::Result<()>;
}

impl AssertUtil for Assert {
    fn try_eq_text(
        &self,
        name: Option<&dyn Display>,
        actual: impl AsRef<str>,
        expected: Data,
    ) -> snapbox::assert::Result<()> {
        let format = likely_format(&expected);
        let expected = text_fallback(expected);
        let actual = actual.as_ref();
        let actual = normalize_path_separators(actual);
        let actual = &*actual;
        if format == DataFormat::TermSvg {
            let rendered = self.redactions().redact(actual.trim_end());
            let rendered = render_svg(&rendered);
            let expected = expected.is(DataFormat::Text);
            self.try_eq(name, rendered.into_data().raw(), expected.raw())
        } else if format == DataFormat::Text {
            let rendered = anstream::adapter::strip_str(actual).to_string();
            let rendered = self.redactions().redact(&rendered);
            self.try_eq(name, rendered.into_data().raw(), expected.raw())
        } else {
            self.try_eq(name, actual.into(), expected)
        }
    }
}

fn text_fallback(data: Data) -> Data {
    if data.format() != DataFormat::Error {
        data
    } else if let Some(path) = data.source().and_then(|source| source.as_path()) {
        Data::read_from(path, Some(DataFormat::Text))
    } else {
        Data::new().is(DataFormat::Text)
    }
}

fn likely_format(data: &Data) -> DataFormat {
    let format = data.format();
    if format == DataFormat::Error {
        let extension = data
            .source()
            .and_then(|source| source.as_path())
            .and_then(|path| path.extension())
            .map(|ext| ext.as_encoded_bytes());
        match extension {
            Some(b"svg") => DataFormat::TermSvg,
            Some(b"txt" | b"md") => DataFormat::Text,
            _ => format,
        }
    } else {
        format
    }
}

pub fn default_assert() -> Assert {
    Assert::new().action_env(DEFAULT_ACTION_ENV)
}

fn normalize_path_separators(text: &str) -> Cow<'_, str> {
    static REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"([\p{L}\p{N}])\\([\p{L}\p{N}])").unwrap());
    REGEX.replace_all(text, "$1/$2")
}

fn render_svg(text: &str) -> String {
    const fn rgb(r: u8, g: u8, b: u8) -> RgbColor {
        RgbColor(r, g, b)
    }

    const BG_COLOR: RgbColor = rgb(26, 26, 26);
    const FG_COLOR: RgbColor = rgb(178, 178, 178);

    const PALETTE: Palette = Palette([
        //
        rgb(54, 60, 70),
        rgb(224, 108, 117),
        rgb(150, 196, 117),
        rgb(209, 154, 102),
        rgb(92, 173, 241),
        rgb(198, 120, 221),
        rgb(81, 181, 195),
        rgb(211, 211, 211),
        //
        rgb(54, 60, 70),
        rgb(224, 108, 117),
        rgb(150, 196, 117),
        rgb(209, 154, 102),
        rgb(92, 173, 241),
        rgb(198, 120, 221),
        rgb(81, 181, 195),
        rgb(211, 211, 211),
        // rgb(110, 112, 116),
        // rgb(224, 108, 117),
        // rgb(168, 220, 131),
        // rgb(244, 183, 127),
        // rgb(95, 183, 255),
        // rgb(224, 135, 251),
        // rgb(94, 211, 227),
        // rgb(250, 250, 250),
    ]);

    anstyle_svg::Term::new()
        .bg_color(BG_COLOR.into())
        .fg_color(FG_COLOR.into())
        .palette(PALETTE)
        .render_svg(text)
        .replace(
            "SFMono-Regular, Consolas, Liberation Mono, Menlo, monospace;",
            "Menlo, Roboto Mono, Ubuntu Mono, Liberation Mono, Consolas, ui-monospace, monospace;",
        )
}
