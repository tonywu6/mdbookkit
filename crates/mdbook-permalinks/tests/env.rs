use std::io::Write;

use anyhow::Result;
use assert_cmd::{Command, prelude::*};
use predicates::prelude::*;
use tap::Pipe;
use tempfile::TempDir;
use tracing::{debug, info, level_filters::LevelFilter};

use mdbookkit::{logging::Logging, testing::setup_paths};

#[test]
fn test_minimum_env() -> Result<()> {
    Logging {
        logging: Some(true),
        colored: Some(false),
        level: LevelFilter::DEBUG,
    }
    .init();

    info!("setup: compile self");
    Command::new("cargo")
        .args(["build", "--package", env!("CARGO_PKG_NAME")])
        .arg(if cfg!(debug_assertions) {
            "--profile=dev"
        } else {
            "--profile=release"
        })
        .assert()
        .success();

    let path = setup_paths()?;

    let root = TempDir::new()?;

    debug!("{root:?}");

    info!("given: a book");
    Command::new("mdbook")
        .args(["init", "--force"])
        .env("PATH", &path)
        .current_dir(&root)
        .unwrap()
        .assert()
        .success();

    info!("given: preprocessor is enabled");
    std::fs::File::options()
        .append(true)
        .open(root.path().join("book.toml"))?
        .pipe(|mut file| file.write_all("[preprocessor.permalinks]\n".as_bytes()))?;

    info!("when: book has path-based links");
    std::fs::File::options()
        .append(true)
        .open(root.path().join("src/chapter_1.md"))?
        .pipe(|mut file| file.write_all("\n[book.toml](../book.toml)\n".as_bytes()))?;

    info!("when: book is not in source control");

    info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("requires a git repository"));

    info!("when: CI=true");

    info!("then: preprocessor fails");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "true")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires a git repository"));

    info!("when: repo has no commit");
    Command::new("git")
        .arg("init")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("No commit found"));

    info!("when: repo has no origin");
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

    info!("then: book builds with warnings");
    Command::new("mdbook")
        .arg("build")
        .env("CI", "false")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Failed to determine the remote URL",
        ));

    info!("when: repo has origin");
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

    info!("then: book builds");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("[WARN]").not())
        .stderr(predicate::str::contains("Using commit hash"));

    info!("when: HEAD is tagged");
    Command::new("git")
        .args(["tag", "v0.1.0", "HEAD"])
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    info!("then: items are linked using tag instead of commit SHA");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("[WARN]").not())
        .stderr(predicate::str::contains("Using tag name \"v0.1.0\""));

    Ok(())
}
