use std::{io::Write, sync::Arc};

use anyhow::{bail, Context, Result};

use assert_cmd::{prelude::*, Command};
use predicates::prelude::*;
use similar::{ChangeTag, TextDiff};
use tap::Pipe;
use tempfile::TempDir;
use tokio::task::JoinSet;

use mdbookkit::bin::rustdoc_link::{
    env::{find_code_extension, Config, Environment},
    Client, Pages, Resolver,
};
use util_testing::{may_skip, portable_snapshots, setup_paths, test_document, TestDocument};

mod util;

async fn snapshot(
    client: Arc<Client>,
    TestDocument { source, name, .. }: TestDocument,
) -> Result<()> {
    let stream = client.env().markdown(source).into_offset_iter();

    let mut page = Pages::one(source, stream)?;

    client.resolve(&mut page).await?;

    let output = page.get(&client.env().emit_config())?.to_string();

    portable_snapshots!().test(|| insta::assert_snapshot!(name.clone(), output))?;

    assert_no_whitespace_change(source, &output)?;

    let report = page
        .reporter()
        .level(log::LevelFilter::Info)
        .names(|_| name.clone())
        .colored(false)
        .logging(false)
        .build()
        .to_report();

    portable_snapshots!().test(|| insta::assert_snapshot!(format!("{name}.stderr"), report))?;

    Ok(())
}

fn assert_no_whitespace_change(source: &str, output: &str) -> Result<()> {
    let changed_lines = TextDiff::from_words(source, output)
        .iter_all_changes()
        .filter_map(|change| {
            if matches!(change.tag(), ChangeTag::Equal) {
                return None;
            }
            if change.value().contains('\n') {
                Some(change.value())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if !changed_lines.is_empty() {
        bail!("unexpected whitespace change: {changed_lines:?}")
    } else {
        Ok(())
    }
}

#[tokio::test]
async fn test_snapshots() -> Result<()> {
    util::setup_logging();

    let client = client()?;

    let tests = [
        test_document!("../../../docs/src/rustdoc-link/supported-syntax.md"),
        test_document!("../../../docs/src/rustdoc-link/known-issues.md"),
        test_document!("../../../docs/src/rustdoc-link/getting-started.md"),
        test_document!("../../../docs/src/rustdoc-link/index.md"),
        test_document!("tests/ra-known-quirks.md"),
    ];

    let errors = tests
        .map(|test| snapshot(client.clone(), test))
        .into_iter()
        .collect::<JoinSet<_>>()
        .join_all()
        .await
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();

    if !errors.is_empty() {
        let errors = errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("{errors}")
    }

    client.drop().await?;

    Ok(())
}

fn client() -> Result<Arc<Client>> {
    Config {
        rust_analyzer: Some("cargo run --package util-rust-analyzer -- analyzer".into()),
        cargo_features: vec!["rustdoc-link".into()],
        ..Default::default()
    }
    .pipe(Environment::new)?
    .pipe(Client::new)
    .pipe(Arc::new)
    .pipe(Ok)
}

#[test]
#[ignore = "should run in a dedicated environment"]
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
            "mdbook-rustdoc-link",
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
        .pipe(|mut file| file.write_all("[preprocessor.rustdoc-link]\n".as_bytes()))?;

    log::info!("when: book is not a Cargo project");
    log::info!("then: preprocessor fails");
    Command::new("mdbook")
        .arg("build")
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "failed to determine the current Cargo project",
        ));

    log::info!("given: book is a Cargo project");
    Command::new("cargo")
        .arg("init")
        .args(["--name", "temp"])
        .env("PATH", &path)
        .current_dir(&root)
        .assert()
        .success();

    if find_code_extension().is_some()
        && may_skip("rust-analyzer code extension is already installed")
    {
        log::info!("when: book has item links");
        std::fs::File::options()
            .append(true)
            .open(root.path().join("src/chapter_1.md"))?
            .pipe(|mut file| file.write_all("\n[std::thread]\n".as_bytes()))?;

        log::info!("then: book builds without errors");
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
        && may_skip("rust-analyzer is already available")
    {
        log::info!("skip testing mdbook build without rust-analyzer")
    } else {
        log::info!("when: rust-analyzer is not configured");

        log::info!("when: book has no item links");

        log::info!("then: book builds without errors");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .success();

        log::info!("when: book has item links");
        std::fs::File::options()
            .append(true)
            .open(root.path().join("src/chapter_1.md"))?
            .pipe(|mut file| file.write_all("\n[std]\n".as_bytes()))?;

        log::info!("then: preprocessor fails");
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

        log::info!("when: code extension is installed");

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
            .args(["run", "--package", "util-rust-analyzer", "--"])
            .arg("--ra-path")
            .arg(ra_executable)
            .arg("download")
            .unwrap()
            .assert()
            .success();

        log::info!("then: book builds without errors");
        Command::new("mdbook")
            .arg("build")
            .env("PATH", &path)
            .current_dir(&root)
            .assert()
            .success();
    }

    Ok(())
}
