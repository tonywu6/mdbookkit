use std::io::Write;

use anyhow::Result;
use assert_cmd::{prelude::*, Command};
use log::LevelFilter;
use predicates::prelude::*;
use tap::Pipe;
use tempfile::TempDir;
use url::Url;

use mdbookkit::{
    bin::link_forever::{Config, Environment, GitHubPermalink, LinkStatus, Pages},
    markdown::mdbook_markdown,
};
use util_testing::{portable_snapshots, setup_paths, test_document, CARGO_WORKSPACE_DIR};

mod util;

#[test]
fn test_snapshots() -> Result<()> {
    util::setup_logging();

    let env = Environment {
        book_src: CARGO_WORKSPACE_DIR.join("crates/mdbookkit/")?,
        vcs_root: CARGO_WORKSPACE_DIR.clone(),
        fmt_link: GitHubPermalink::new("lorem", "ipsum", "dolor").pipe(Box::new),
        markdown: mdbook_markdown(),
        config: Config {
            book_url: Some("https://example.org/book".parse::<Url>()?.into()),
            ..Default::default()
        },
    };

    let mut pages = Pages::new(mdbook_markdown());

    let tests = [
        test_document!("tests/ra-known-quirks.md"), // only for providing anchors
        test_document!("tests/link-forever.md"),
    ];

    for page in tests.iter() {
        pages.insert(page.file.clone(), page.source)?;
    }

    env.resolve(&mut pages);

    for page in tests {
        let output = pages.emit(&page.file)?;
        let name = page.name;
        portable_snapshots!().test(|| insta::assert_snapshot!(format!("{name}"), output))?;
    }

    macro_rules! assert_stderr {
        ($status:pat, $snap:literal) => {
            let report = env
                .report(&pages, |status| matches!(status, $status))
                .level(LevelFilter::Debug)
                .names(|url| env.rel_path(url))
                .colored(false)
                .logging(false)
                .build()
                .to_report();
            portable_snapshots!().test(|| insta::assert_snapshot!($snap, report))?;
        };
    }

    assert_stderr!(LinkStatus::Ignored, "_stderr.ignored");
    assert_stderr!(LinkStatus::Published, "_stderr.published");
    assert_stderr!(LinkStatus::Rewritten, "_stderr.rewritten");
    assert_stderr!(LinkStatus::Permalink, "_stderr.permalink");
    assert_stderr!(LinkStatus::PathNotCheckedIn, "_stderr.not-checked-in");
    assert_stderr!(LinkStatus::NoSuchPath, "_stderr.no-such-path");
    assert_stderr!(LinkStatus::NoSuchFragment, "_stderr.no-such-fragment");
    assert_stderr!(LinkStatus::Error(..), "_stderr.link-error");

    Ok(())
}

#[test]
fn test_minimum_env() -> Result<()> {
    util::setup_logging();

    log::info!("setup: compile self");
    Command::new("cargo")
        .args([
            "build",
            "--package",
            env!("CARGO_PKG_NAME"),
            "--all-features",
            "--bin",
            "mdbook-link-forever",
        ])
        .arg(if cfg!(debug_assertions) {
            "--profile=dev"
        } else {
            "--profile=release"
        })
        .assert()
        .success();

    let path = setup_paths()?;

    let root = TempDir::new()?;

    log::debug!("{root:?}");

    log::info!("given: a book");
    Command::new("mdbook")
        .args(["init", "--force"])
        .env("PATH", &path)
        .current_dir(&root)
        .unwrap()
        .assert()
        .success();

    log::info!("given: preprocessor is enabled");
    std::fs::File::options()
        .append(true)
        .open(root.path().join("book.toml"))?
        .pipe(|mut file| file.write_all("[preprocessor.link-forever]\n".as_bytes()))?;

    log::info!("when: book has path-based links");
    std::fs::File::options()
        .append(true)
        .open(root.path().join("src/chapter_1.md"))?
        .pipe(|mut file| file.write_all("\n[book.toml](../book.toml)\n".as_bytes()))?;

    log::info!("when: book is not in source control");

    log::info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("requires a git repository"));

    log::info!("when: CI=true");

    log::info!("then: preprocessor fails");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "true")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a git repository"));

    log::info!("when: repo has no commit");
    Command::new("git")
        .arg("init")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    log::info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("no commit found"));

    log::info!("when: repo has no origin");
    Command::new("git")
        .args(["commit", "--allow-empty"])
        .args(["--message", "init"])
        .env("PATH", &path)
        .env("GIT_AUTHOR_NAME", "me")
        .env("GIT_AUTHOR_EMAIL", "me@example.org")
        .env("GIT_COMMITTER_NAME", "me")
        .env("GIT_COMMITTER_EMAIL", "me@example.org")
        .current_dir(&root)
        .assert()
        .success();

    log::info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("failed to determine GitHub url"));

    log::info!("when: repo has origin");
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/lorem/ipsum.git",
        ])
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    log::info!("then: book builds");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("[WARN]").not())
        .stderr(predicate::str::contains("using commit"));

    log::info!("when: HEAD is tagged");
    Command::new("git")
        .args(["tag", "v0.1.0", "HEAD"])
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    log::info!("then: items are linked using tag instead of commit SHA");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("[WARN]").not())
        .stderr(predicate::str::contains("using tag \"v0.1.0\""));

    Ok(())
}
