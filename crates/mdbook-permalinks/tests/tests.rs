use anyhow::{Context, Result, anyhow};

use mdbookkit_testing::{
    TestBook, TestRoot,
    camino::Utf8Path,
    regex::Regex,
    snapbox::{
        RedactedValue,
        cmd::Command,
        dir::{DirFixture, DirRoot},
    },
    test_mdbook,
};
use tap::TryConv;

#[allow(unused)]
macro_rules! test_case {
    [ $(#[$attr:meta])* $name:ident, $($args:tt)+] => {
        #[test]
        $(#[$attr])*
        fn $name() -> Result<()> {
            test_mdbook![$name, $($args)+, redacted = [redacted()]];
            let test = $name()?;
            std::fs::write(test.path.book_dir().join("src/ignored.txt"), "")?;
            std::fs::write(test.path.book_dir().join("src/ignored.rs"), "")?;
            test.run()
        }
    };
}

test_case![file_links, exit(0)];
test_case![repo_links, exit(0)];
test_case![book_links, exit(0)];

test_case![git_url_from_book, exit(0)];
test_case![git_url_scp_like, exit(0)];
test_case![git_url_unsupported, exit(101)];
test_case![git_url_with_query, exit(0)];
test_case![git_url_codeberg, exit(0)];
test_case![git_url_tangled, exit(0)];
test_case![git_url_tangled_did, exit(0)];
test_case![git_url_tangled_self_hosted, exit(101)];
test_case![git_url_tangled_malformed, exit(101)];
test_case![git_url_custom_params, exit(0)];
test_case![git_url_invalid_config, exit(101)];

test_case![ambiguous_paths, exit(0)];
test_case![path_encoding, exit(0)];
test_case![
    #[cfg_attr(windows, ignore)]
    path_encoding_unix,
    exit(0)
];
test_case![
    #[cfg_attr(not(windows), ignore)]
    path_encoding_windows,
    exit(0)
];
test_case![site_url_absolute_paths, exit(0)];
test_case![site_url_invalid, exit(101)];
test_case![site_url_path_encoding, exit(0)];
test_case![dev_mode, exit(0), env = ["CI" = ""]];

test_case![
    book_tutorial_check,
    exit(0),
    env = ["MDBOOKKIT_TERM_GRAPHICAL" = "unicode"]
];
test_case![
    book_hardcoded_repo_link,
    exit(0),
    env = ["MDBOOKKIT_TERM_GRAPHICAL" = "unicode"]
];
test_case![
    book_hardcoded_book_link_not_found,
    exit(0),
    env = ["MDBOOKKIT_TERM_GRAPHICAL" = "unicode"]
];

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

test_in_temp_dir![git_no_commit(exit(0), env = ["CI" = ""]), |book| {
    git_no_commit_test(book)
}];

test_in_temp_dir![git_no_commit_in_ci(exit(101), env = ["CI" = "1"]), |book| {
    git_no_commit_test(book)
}];

fn git_no_commit_test(book: &TestRoot<'static>) -> Result<()> {
    run!(book, "git", "init");
    #[rustfmt::skip]
    run!(book, "git", "remote", "add", "origin", "https://github.com/lorem/ipsum.git");
    Ok(())
}

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
        .unwrap()
        .try_conv::<&Utf8Path>()?
        .to_owned();
    book.path.root_dir = root_dir;

    (template.book_dir().as_std_path())
        .write_to_path(book.path.book_dir().as_std_path())
        .context("failed to initialize temp dir")?;

    setup(&book.path)?;

    book.run()?;

    match book.path.dist_dir().as_std_path().canonicalize() {
        Ok(path) => path
            .write_to_path(template.dist_dir().as_std_path())
            .map_err(<_>::into),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => anyhow::Ok(()),
        Err(err) => Err(anyhow!(err)),
    }
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
            Regex::new(r"(?<redacted>[0-9a-f]{40}|v[0-9.]+)")
                .unwrap()
                .into(),
        ),
        (
            "[CARGO_PKG_REPOSITORY]",
            env!("CARGO_PKG_REPOSITORY").into(),
        ),
    ]
}
