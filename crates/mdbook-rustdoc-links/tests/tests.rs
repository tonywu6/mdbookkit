use mdbookkit_testing::{snapbox::file, test_mdbook};

test_mdbook![
    rustdoc,
    exit(0),
    stderr.svg = file!["rustdoc/stderr/data.svg": TermSvg],
    stderr.txt = file!["rustdoc/stderr/data.txt": Text],
    rendered = [file!["rustdoc/out/chapter_1.md": Text]],
];
