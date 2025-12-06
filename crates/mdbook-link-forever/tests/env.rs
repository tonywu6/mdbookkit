use std::io::Write;

use anyhow::Result;
use assert_cmd::{Command, prelude::*};
use predicates::prelude::*;
use tap::Pipe;
use tempfile::TempDir;

use mdbookkit::testing::{setup_logging, setup_paths};

#[test]
fn test_minimum_env() -> Result<()> {
    setup_logging(env!("CARGO_PKG_NAME"));

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
