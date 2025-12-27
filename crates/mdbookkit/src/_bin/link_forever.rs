use std::process::exit;

use console::style;

fn main() {
    eprintln!("Binaries from the `mdbookkit` package have been moved.");
    eprintln!(
        "You can reinstall it via the command {}",
        style("cargo install mdbook-permalinks")
            .for_stderr()
            .bold()
            .blue()
    );
    eprintln!(
        "Note that the executable name has been changed to `{}` (from `{}`)",
        style("mdbook-permalinks")
            .for_stderr()
            .bold()
            .bright()
            .white(),
        style("mdbook-link-forever").for_stderr().strikethrough()
    );
    exit(2);
}
