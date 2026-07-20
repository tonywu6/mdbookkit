use std::path::{Path, PathBuf};

use anyhow::Result;
use camino::Utf8PathBuf;
use tap::{Tap, TryConv};

use mdbookkit_testing::TestRoot;

fn main() -> Result<()> {
    match clap::Parser::parse() {
        Program::Summary { book } => summary(&book),
    }
}

fn summary(book: &Path) -> Result<()> {
    let root_dir = book.to_owned().try_conv::<Utf8PathBuf>()?;
    let name = root_dir.file_name().unwrap();
    let root_dir = root_dir.parent().unwrap().to_owned();

    let book = TestRoot { name, root_dir };

    let book = book
        .expected_pages()?
        .collect::<Result<Vec<_>>>()?
        .tap_mut(|book| book.sort());

    for page in book.iter() {
        eprintln!("{}", page.toc_item())
    }

    eprintln!();

    for page in book.iter() {
        eprintln!("{}", page.mod_item())
    }

    eprintln!();

    Ok(())
}

#[derive(clap::Parser, Debug)]
enum Program {
    Summary { book: PathBuf },
}
