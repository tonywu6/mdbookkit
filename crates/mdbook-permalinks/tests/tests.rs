use anyhow::{Context, Result};

use mdbookkit_testing::{
    TestBook, TestRoot,
    camino::{Utf8Path, Utf8PathBuf},
    regex::Regex,
    snapbox::{
        RedactedValue,
        cmd::Command,
        dir::{DirFixture, DirRoot},
        utils::current_dir,
    },
    test_mdbook,
};
use tap::{Conv, TryConv};

macro_rules! test_case {
    [$name:ident, $($args:tt)+] => {
        #[test]
        fn $name() -> Result<()> {
            test_mdbook![$name, $($args)+, redacted = [redacted()]];
            $name()?.run()
        }
    };
}

test_case![file_links, exit(0)];
test_case![http_links, exit(0)];

macro_rules! test_in_temp_dir {
    [$name:ident ($($args:tt)+), |$root:ident| { $($setup:tt)* }] => {
        #[test]
        fn $name() -> Result<()> {
            test_mdbook![$name, $($args)+, redacted = [redacted()]];
            temp_dir_test($name()?, |#[allow(unused)] $root| { $($setup)* })
        }
    };
}

macro_rules! run {
    ( $book:ident $(, [$env:literal = $var:literal])*, $command:literal $(, $arg:literal)* ) => {
        (Command::new($command).args([$($arg),*]))
            .current_dir($book.book_dir())
            $(.env($env, $var))*
            .assert()
            .success();
    };
}

test_in_temp_dir![git_no_repo(exit(0)), |book| { Ok(()) }];

test_in_temp_dir![git_no_commit(exit(0)), |book| {
    run!(book, "git", "init");
    Ok(())
}];

test_in_temp_dir![git_no_remote(exit(0)), |book| {
    run!(book, "git", "init");
    #[rustfmt::skip]
    run!(
        book,
        ["GIT_AUTHOR_NAME" = "me"],
        ["GIT_AUTHOR_EMAIL" = "me@example.org"],
        ["GIT_COMMITTER_NAME" = "me"],
        ["GIT_COMMITTER_EMAIL" = "me@example.org"],
        "git", "commit", "--allow-empty", "--message", "init"
    );
    Ok(())
}];

test_in_temp_dir![
    git_tag(
        exit(0),
        env = ["MDBOOK_LOG" = "warn,mdbook_permalinks=info"]
    ),
    |book| {
        run!(book, "git", "init");
        #[rustfmt::skip]
        run!(
            book,
            ["GIT_AUTHOR_NAME" = "me"],
            ["GIT_AUTHOR_EMAIL" = "me@example.org"],
            ["GIT_COMMITTER_NAME" = "me"],
            ["GIT_COMMITTER_EMAIL" = "me@example.org"],
            "git", "commit", "--allow-empty", "--message", "init"
        );
        #[rustfmt::skip]
        run!(book, "git", "remote", "add", "origin", "https://github.com/lorem/ipsum.git");
        run!(book, "git", "tag", "v0.1.0", "HEAD");
        Ok(())
    }
];

fn temp_dir_test<F>(mut book: TestBook, setup: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a TestRoot<'static>) -> Result<()>,
{
    let template = book.path.clone();

    let root_dir = DirRoot::mutable_temp()?;
    let root_dir = (root_dir.path())
        .expect("temp dir should have a path")
        .try_conv::<&Utf8Path>()?
        .to_owned();
    book.path.root_dir = root_dir;

    (template.book_dir().as_std_path())
        .write_to_path(book.path.book_dir().as_std_path())
        .context("failed to initialize temp dir")?;

    {
        let package_dir = current_dir!()
            .join("..")
            .canonicalize()?
            .try_conv::<Utf8PathBuf>()?;
        let cargo_conf_path = book.path.book_dir().join(".cargo/config.toml");
        let cargo_conf = std::fs::read_to_string(&cargo_conf_path)?
            .replace(r#""${PACKAGE_DIR}""#, &toml_str(&package_dir))
            .replace(
                r#""${PACKAGE_DIR}/Cargo.toml""#,
                &toml_str(package_dir.join("Cargo.toml")),
            );
        std::fs::write(cargo_conf_path, cargo_conf)?;
    }

    setup(&book.path)?;

    book.run()?;

    (book.path.dist_dir().as_std_path())
        .write_to_path(template.dist_dir().as_std_path())
        .context("failed to sync snapshots")?;

    (book.path.stderr_dir().as_std_path())
        .write_to_path(template.stderr_dir().as_std_path())
        .context("failed to sync snapshots")?;

    Ok(())
}

fn redacted() -> Vec<(&'static str, RedactedValue)> {
    vec![
        (
            "[GIT_REVISION]",
            Regex::new(r"(tree|blob|raw)/(?<redacted>[0-9a-f]{40}|v.+?)/")
                .unwrap()
                .into(),
        ),
        (
            "[CARGO_PKG_REPOSITORY]",
            env!("CARGO_PKG_REPOSITORY").into(),
        ),
    ]
}

fn toml_str<S: AsRef<str>>(s: S) -> String {
    s.as_ref().conv::<toml::Value>().to_string()
}
