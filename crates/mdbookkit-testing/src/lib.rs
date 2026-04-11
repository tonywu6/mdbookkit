use std::path::PathBuf;

use anstream::adapter::strip_bytes;
use anyhow::Result;
use regex::Regex;
use snapbox::{Assert, Data, Redactions, assert::DEFAULT_ACTION_ENV, cmd::Command, dir::DirRoot};

pub use anyhow;
pub use snapbox;

pub struct TestBook<'a> {
    pub name: &'a str,
    pub root_dir: PathBuf,
    pub env_vars: Vec<(&'a str, &'a str)>,
    pub stderr_txt: Data,
    pub stderr_svg: Data,
}

impl TestBook<'_> {
    pub fn run(self) -> Result<()> {
        let Self {
            name,
            root_dir,
            env_vars,
            stderr_txt,
            stderr_svg,
        } = self;

        let book_dir = root_dir.join(name);
        let temp_dir = DirRoot::mutable_temp()?;

        let assert = {
            let mut redactions = Redactions::new();
            redactions.insert("[TEST_DIR]", &root_dir)?;
            redactions.insert(
                "[CARGO_DURATION]",
                Regex::new(r"target\(s\) in (?<redacted>\d+\.\d+s)")?,
            )?;
            Assert::new()
                .action_env(DEFAULT_ACTION_ENV)
                .redact_with(redactions)
        };

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
            .env("MDBOOK_LOG", "info,mdbookkit::diagnostics=debug")
            .env("MDBOOKKIT_TERM_GRAPHICAL", "ascii")
            .env("FORCE_COLOR", "1")
            .envs(env_vars)
            .assert()
            .success();

        let stderr_svg_actual = &*result.get_output().stderr;
        let stderr_txt_actual = strip_bytes(stderr_svg_actual).into_vec();

        assert.eq(stderr_txt_actual, stderr_txt);
        assert.eq(stderr_svg_actual, stderr_svg);

        Ok(())
    }
}

#[macro_export]
macro_rules! test_mdbook {
    [
        $name:ident,
        stderr.svg = $stderr_svg:expr,
        stderr.txt = $stderr_txt:expr,
        $( env = [$( $env_key:literal = $env_val:literal ),*], )?
    ] => {
        #[test]
        fn $name() -> $crate::anyhow::Result<()> {
            $crate::TestBook {
                name: stringify!($name),
                root_dir: $crate::snapbox::current_dir!(),
                stderr_svg: $stderr_svg,
                stderr_txt: $stderr_txt,
                env_vars: vec![$($(($env_key, $env_val))*)?],
            }
            .run()
        }
    };
}
