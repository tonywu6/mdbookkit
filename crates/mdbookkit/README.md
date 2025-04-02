# mdbookkit

![mdbookkit hero image](/docs/src/media/banner.webp)

Quality-of-life plugins for your [mdBook] project.

Right now, there are two mdBook [preprocessors], both for generating **correct and
versioned** external links from **easy-to-write** markup.

- [**`mdbook-rustdoc-link`**](https://tonywu6.github.io/mdbookkit/rustdoc-link)

  _rustdoc_-style linking for Rust APIs: write types and function names, get links to
  _docs.rs_

- [**`mdbook-link-forever`**](https://tonywu6.github.io/mdbookkit/link-forever)

  _Permalinks_ for your source tree: write relative paths, get links to GitHub.

> [!TIP]
>
> Preprocessors are standalone programs that mdBook invokes to transform your Markdown
> sources before rendering them.

## Installation

If you are interested in any of these plugins, visit their respective pages for usage
instructions, linked above.

If you want to install all of them:

```bash
cargo install mdbookkit --all-features
```

Precompiled binaries are also available from [GitHub releases][gh-releases].

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

<!-- prettier-ignore-start -->

[mdBook]: https://rust-lang.github.io/mdBook/
[`mdbookkit`]: https://crates.io/crates/mdbookkit
[preprocessors]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases

<!-- prettier-ignore-end -->
