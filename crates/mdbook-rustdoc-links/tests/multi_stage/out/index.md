- [`crate::amd64_only`](https://docs.rs/multi_stage/0.1.0/multi_stage/fn.amd64_only.html "fn multi_stage::amd64_only")
- [`crate::arm64_only`](https://docs.rs/multi_stage/0.1.0/multi_stage/fn.arm64_only.html "fn multi_stage::arm64_only")
- [`anstyle_parse::Utf8Parser`](https://docs.rs/anstyle-parse/1.0.0/anstyle_parse/struct.Utf8Parser.html "struct anstyle_parse::Utf8Parser")
- [`utf8_width::get_width`]
  - This link should not resolve because the package is not selected by `build.packages`
    in any build stage.
- [`crate::get_width`](https://example.org/utf8_width/fn.get_width.html "fn utf8_width::get_width")
  - This link should resolve because of the `--extern-html-root-url` option: even though
    the package is not selected by `build.packages` and thus will not have HTML files,
    the item is resolvable because it is re-exported.
- [`utf8parse::Parser`]
  - This link should not resolve because the package is a transitive dependency of
    `anstyle-parse` and is therefore not selected by `build.packages`, even though it
    will be built as necessary by `cargo doc`.
