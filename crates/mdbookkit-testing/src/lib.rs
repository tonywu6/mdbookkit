use std::{borrow::Cow, fmt::Display, path::PathBuf, sync::LazyLock};

use anstyle::RgbColor;
use anstyle_svg::Palette;
use anyhow::{Context, Result};
use regex::Regex;
use snapbox::{
    Assert, Data, IntoData, RedactedValue, Redactions, assert::DEFAULT_ACTION_ENV, cmd::Command,
    data::DataFormat, dir::DirRoot,
};

pub use anyhow;
pub use regex;
pub use snapbox;

pub struct TestBook {
    pub name: &'static str,
    pub code: i32,
    pub root_dir: PathBuf,
    pub env_vars: Vec<(&'static str, &'static str)>,
    pub redacted: Vec<(&'static str, RedactedValue)>,
    pub stderr_txt: Data,
    pub stderr_svg: Data,
    pub rendered: Vec<Data>,
}

impl TestBook {
    pub fn run(self) -> Result<()> {
        let Self {
            name,
            code,
            root_dir,
            env_vars,
            redacted,
            stderr_txt,
            stderr_svg,
            rendered,
        } = self;

        let book_dir = root_dir.join(name);
        let temp_dir = DirRoot::mutable_temp()?;
        let dist_dir = book_dir.join("out");

        Command::new(env!("CARGO"))
            .arg("clean")
            .current_dir(&book_dir)
            .assert()
            .success();

        let result = Command::new(env!("CARGO"))
            .args(["bin", "mdbook", "build"])
            .arg(&book_dir)
            .current_dir(&root_dir)
            .env("MDBOOK_build__build_dir", temp_dir.path().unwrap())
            .env("MDBOOK_LOG", "off,mdbookkit::diagnostics=info")
            .env("MDBOOKKIT_TERM_GRAPHICAL", "ascii")
            .env("FORCE_COLOR", "1")
            .env("RUST_BACKTRACE", "0")
            .envs(env_vars)
            .assert()
            .code(code);

        let assert = {
            let mut redactions = Redactions::new();
            redactions.insert("[TEST_DIR]", &root_dir)?;
            redactions.insert("[ELAPSED]", Regex::new(r"in (?<redacted>\d+\.\d+s)")?)?;
            redactions.insert(
                "[LLVM_COV_STDERR]",
                Regex::new(r"(?<redacted>error: process didn't exit successfully:.*)")?,
            )?;
            for (placeholder, matcher) in redacted {
                redactions.insert(placeholder, matcher)?;
            }
            Assert::new()
                .action_env(DEFAULT_ACTION_ENV)
                .redact_with(redactions)
        };

        let stderr = &*result.get_output().stderr;
        let stderr = String::from_utf8_lossy(stderr);

        let mut results = vec![
            assert.try_eq_text(None, &stderr, stderr_txt),
            assert.try_eq_text(None, &stderr, stderr_svg),
        ];

        for expected in rendered {
            let page = expected.source().unwrap().as_path().unwrap();
            let page = page.strip_prefix(&dist_dir)?.to_owned();

            let actual_path = temp_dir.path().unwrap().join(&page);
            let actual = std::fs::read_to_string(actual_path)
                .with_context(|| format!("no such page: {:?}", page.display()))?;

            results.push(assert.try_eq_text(Some(&page.display()), actual, expected));
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
}

trait AssertUtil {
    fn try_eq_text<S: AsRef<str>>(
        &self,
        name: Option<&dyn Display>,
        actual: S,
        expected: Data,
    ) -> snapbox::assert::Result<()>;
}

impl AssertUtil for Assert {
    fn try_eq_text<S: AsRef<str>>(
        &self,
        name: Option<&dyn Display>,
        actual: S,
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
        rgb(54, 60, 70),
        rgb(224, 108, 117),
        rgb(150, 196, 117),
        rgb(209, 154, 102),
        rgb(92, 173, 241),
        rgb(198, 120, 221),
        rgb(81, 181, 195),
        rgb(211, 211, 211),
        rgb(110, 112, 116),
        rgb(224, 108, 117),
        rgb(168, 220, 131),
        rgb(244, 183, 127),
        rgb(95, 183, 255),
        rgb(224, 135, 251),
        rgb(94, 211, 227),
        rgb(250, 250, 250),
    ]);

    anstyle_svg::Term::new()
        .bg_color(BG_COLOR.into())
        .fg_color(FG_COLOR.into())
        .palette(PALETTE)
        .render_svg(text)
        .replace(
            "SFMono-Regular, Consolas, Liberation Mono, Menlo, monospace;",
            "Menlo, SF Mono, Liberation Mono, Consolas, ui-monospace, monospace;",
        )
}

#[macro_export]
macro_rules! test_mdbook {
    [
        $name:ident,
        exit($code:literal),
        stderr.svg = $stderr_svg:expr,
        stderr.txt = $stderr_txt:expr,
        $( rendered = [$($data:expr),*], )?
        $( env = [$($env:tt)*], )?
        $( redacted = [$($redacted:tt)*], )?
    ] => {
        #[test]
        fn $name() -> $crate::anyhow::Result<()> {
            $crate::TestBook {
                name: stringify!($name),
                code: $code,
                root_dir: $crate::snapbox::current_dir!(),
                stderr_svg: $stderr_svg,
                stderr_txt: $stderr_txt,
                rendered: vec![$($($data),*)?],
                env_vars: $crate::test_mdbook!(@key_values $($($env)*)?),
                redacted: $crate::test_mdbook!(@key_values $($($redacted)*)?),
            }
            .run()
        }
    };

    (@key_values $($key:literal = $val:expr),*) => {
        vec![$(($key, $val)),*]
    };
    (@key_values $($tt:tt)+) => {
        $($tt)+
    }
}
