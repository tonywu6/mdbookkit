use mdbookkit_testing::{snapbox::file, test_mdbook};

test_mdbook![
    out_of_the_box,
    stderr.svg = file!["out_of_the_box/stderr/data.svg": TermSvg],
    stderr.txt = file!["out_of_the_box/stderr/data.txt": Text],
    rendered = [file!["out_of_the_box/out/chapter_1.md": Text]],
];
