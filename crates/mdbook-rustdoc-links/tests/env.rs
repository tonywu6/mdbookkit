use std::io::Write;

use anyhow::{Context, Result};
use assert_cmd::{Command, assert::OutputAssertExt};
use predicates::prelude::*;
use tap::Pipe;
use tempfile::TempDir;
use tracing::{debug, info, level_filters::LevelFilter};

use mdbookkit::{
    logging::Logging,
    testing::{not_in_ci, setup_paths},
};

#[test]
#[ignore = "should run in a dedicated environment"]
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
        .pipe(|mut file| file.write_all("[preprocessor.rustdoc-links]\n".as_bytes()))?;

    info!("when: book is not a Cargo project");
    info!("then: preprocessor fails");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Failed to determine the current Cargo project",
        ));

    info!("given: book is a Cargo project");
    Command::new("cargo")
        .arg("init")
        .args(["--name", "temp"])
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    if Command::new("mdbook-rustdoc-links")
        .arg("rust-analyzer")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .try_stdout(predicate::str::contains("VS Code extension"))
        .is_ok()
        && not_in_ci("rust-analyzer code extension is already installed")
    {
        info!("when: book has item links");
        std::fs::File::options()
            .append(true)
            .open(root.path().join("src/chapter_1.md"))?
            .pipe(|mut file| file.write_all("\n[std::thread]\n".as_bytes()))?;

        info!("then: book builds without errors");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .success();
    } else if Command::new("rust-analyzer")
        .arg("--version")
        .assert()
        .try_success()
        .is_ok()
        && not_in_ci("rust-analyzer is already available")
    {
        info!("skip testing mdbook build without rust-analyzer")
    } else {
        info!("when: rust-analyzer is not configured");

        info!("when: book has no item links");

        info!("then: book builds without errors");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .success();

        info!("when: book has item links");
        std::fs::File::options()
            .append(true)
            .open(root.path().join("src/chapter_1.md"))?
            .pipe(|mut file| file.write_all("\n[std]\n".as_bytes()))?;

        info!("then: preprocessor fails");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("failed to spawn rust-analyzer")
                    // https://github.com/rust-lang/rustup/issues/3846
                    // rustup shims rust-analyzer when it's not installed
                    .or(predicate::str::contains("Unknown binary 'rust-analyzer")),
                // ^ doesn't have a closing `'` because on windows it says 'rust-analyzer.exe'
            );

        info!("when: code extension is installed");

        let extension_dir = tempfile::Builder::new()
            .prefix(".vscode")
            .suffix("")
            .rand_bytes(0)
            .tempdir_in(dirs::home_dir().context("failed to get home dir")?)?;

        let ra_executable = extension_dir
            .path()
            .join("extensions/rust-lang.rust-analyzer-lorem-ipsum")
            .join("server/rust-analyzer");

        Command::new("cargo")
            .args(["xtask", "rust-analyzer"])
            .arg("--ra-path")
            .arg(ra_executable)
            .arg("download")
            .unwrap()
            .assert()
            .success();

        info!("then: book builds without errors");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .success();
    }

    Ok(())
}
