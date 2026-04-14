use std::{borrow::Cow, ffi::OsStr, fmt::Display, path::Path, sync::LazyLock};

use anstyle::RgbColor;
use anstyle_svg::Palette;
use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;
use snapbox::{
    Assert, Data, IntoData, RedactedValue, Redactions,
    assert::DEFAULT_ACTION_ENV,
    cmd::Command,
    data::DataFormat,
    dir::{DirRoot, Walk},
};
use tap::TryConv;

pub use anyhow;
pub use regex;
pub use snapbox;

pub struct TestBook {
    pub path: TestRoot,
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

        self.cargo("clean", self.path.book_dir());

        let result = self
            .cargo("bin", &self.path.root_dir)
            .current_dir(&self.path.root_dir)
            .args(["mdbook", "build"])
            .arg(self.path.book_dir())
            .env("MDBOOK_build__build_dir", temp_dir)
            .envs(load_env(&[
                ("MDBOOK_LOG", "off,mdbookkit::diagnostics=info"),
                ("MDBOOKKIT_TERM_GRAPHICAL", "ascii"),
                ("FORCE_COLOR", "1"),
                ("RUST_BACKTRACE", "0"),
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
            assert.try_eq_text(None, &stderr, stderr_txt),
            assert.try_eq_text(None, &stderr, stderr_svg),
        ];

        for page in self.path.expected_pages() {
            let (name, expected) = page?;
            let actual = self.path.actual_page(&name, temp_dir)?;
            results.push(assert.try_eq_text(Some(&name), actual, expected));
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
        Ok(Assert::new()
            .action_env(DEFAULT_ACTION_ENV)
            .redact_with(redactions))
    }

    pub fn cargo(&self, command: &str, wd: impl AsRef<Path>) -> Command {
        Command::new(env!("CARGO")).arg(command).current_dir(wd)
    }
}

pub struct TestRoot {
    pub root_dir: Utf8PathBuf,
    pub name: &'static str,
}

impl TestRoot {
    pub fn book_dir(&self) -> Utf8PathBuf {
        self.root_dir.join(self.name)
    }

    pub fn dist_dir(&self) -> Utf8PathBuf {
        self.book_dir().join("out")
    }

    fn stderr_txt(&self) -> Data {
        self.test_data("stderr/data.txt", DataFormat::Text)
    }

    fn stderr_svg(&self) -> Data {
        self.test_data("stderr/data.svg", DataFormat::TermSvg)
    }

    pub fn expected_pages(&self) -> impl Iterator<Item = Result<(Utf8PathBuf, Data)>> {
        let root = self.book_dir();

        Walk::new(root.join("src").as_std_path())
            .map(|path| {
                let path = path
                    .context("error walking src dir")?
                    .try_conv::<Utf8PathBuf>()?;

                if path.extension() != Some("md") || path.file_name() == Some("SUMMARY.md") {
                    return Ok(None);
                }

                let root = self.book_dir();
                let name = path
                    .strip_prefix(root.join("src"))
                    .expect("path is under src dir")
                    .to_owned();

                let path = root.join("out").join(&name);
                let data = self.test_data(path, DataFormat::Text);

                Ok(Some((name, data)))
            })
            .filter_map(Result::transpose)
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

fn load_env<'a>(vars: &[(&'a str, &str)]) -> impl Iterator<Item = (&'a str, impl AsRef<OsStr>)> {
    vars.iter().map(|(key, default)| {
        let val = if let Some(overridden) = std::env::var_os(key) {
            eprintln!(
                "--- overriding env var {key:?} = {:?}",
                &*overridden.to_string_lossy()
            );
            Cow::Owned(overridden)
        } else {
            Cow::Borrowed(default.as_ref())
        };
        (*key, val)
    })
}

#[macro_export]
macro_rules! test_mdbook {
    [
        @init $name:ident exit($code:literal)
        $( , env = [$($env:tt)*] )?
        $( , redacted = [$($redacted:tt)*] )?
    ] => {
        $crate::TestBook {
            path: $crate::TestRoot {
                name: stringify!($name),
                root_dir: $crate::snapbox::current_dir!().try_into()?,
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
        let actual = actual.as_ref();
        let actual = normalize_path_separators(actual);
        let actual = &*actual;
        if expected.format() == DataFormat::TermSvg {
            let rendered = self.redactions().redact(actual.trim_end());
            let rendered = render_svg(&rendered);
            let expected = expected.coerce_to(DataFormat::Text);
            self.try_eq(name, rendered.into_data().raw(), expected.raw())
        } else if expected.format() == DataFormat::Text {
            let rendered = anstream::adapter::strip_str(actual).to_string();
            self.try_eq(name, rendered.into_data().raw(), expected.raw())
        } else {
            self.try_eq(name, actual.into(), expected)
        }
    }
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
