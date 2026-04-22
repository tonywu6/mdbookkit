use mdbookkit_testing::{regex::Regex, snapbox::RedactedValue, test_mdbook};

test_mdbook![file_links, exit(0), redacted = [redacted()]];

fn redacted() -> Vec<(&'static str, RedactedValue)> {
    vec![
        (
            "[GIT_REVISION]",
            Regex::new(r"(tree|blob|raw)/(?<redacted>[0-9a-f]{40}|v.+?)/")
                .unwrap()
                .into(),
        ),
        (
            "[CARGO_PKG_REPOSITORY]",
            env!("CARGO_PKG_REPOSITORY").into(),
        ),
    ]
}
