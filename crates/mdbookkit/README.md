# mdbookkit

![mdbookkit hero image](https://github.com/tonywu6/mdbookkit/raw/HEAD/docs/src/media/banner.webp)

[![crates.io](https://img.shields.io/crates/v/mdbookkit?style=flat-square)](https://crates.io/crates/mdbookkit)
[![documentation](https://img.shields.io/github/actions/workflow/status/tonywu6/mdbookkit/docs.yml?event=release&style=flat-square&label=docs)](https://tonywu6.github.io/mdbookkit/)
[![MIT/Apache-2.0 licensed](https://img.shields.io/crates/l/mdbookkit?style=flat-square)](https://github.com/tonywu6/mdbookkit/tree/main/LICENSE-APACHE.md)

Quality-of-life plugins for your [mdBook] project.

- [**`mdbook-rustdoc-link`**](https://tonywu6.github.io/mdbookkit/rustdoc-link)

  _rustdoc_-style linking for Rust APIs: write types and function names, get links to
  _docs.rs_

- [**`mdbook-link-forever`**](https://tonywu6.github.io/mdbookkit/link-forever)

  _Permalinks_ for your source tree: write relative paths, get links to GitHub.

## Installation

If you are interested in any of these plugins, visit their respective pages for usage
instructions, linked above.

If you want to install all of them:

```bash
cargo install mdbookkit --all-features
```

Precompiled binaries are also available from [GitHub releases][gh-releases].

## License

This project is released under the
[Apache 2.0 License](https://github.com/tonywu6/mdbookkit/tree/main/LICENSE-APACHE.md)
and the [MIT License](https://github.com/tonywu6/mdbookkit/tree/main/LICENSE-MIT.md).

<!-- prettier-ignore-start -->

[`mdbookkit`]: https://crates.io/crates/mdbookkit
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessors]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html

<!-- prettier-ignore-end -->
