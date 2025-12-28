# mdbookkit

Support library for [mdBook] [preprocessors] in the
[`mdbookkit`](https://github.com/tonywu6/mdbokkkit) project.

You may be looking for:

- [`mdbook-rustdoc-links`](https://docs.tonywu.dev/mdbookkit/rustdoc-links/)
- [`mdbook-permalinks`](https://docs.tonywu.dev/mdbookkit/permalinks/)

> [!NOTE]
>
> This package previously provided preprocessor binaries. The preprocessors are now
> published as standalone packages. You should no longer install this package:
>
> ```sh
> cargo uninstall mdbookkit
> # cargo install mdbook-rustdoc-links
> # cargo install mdbook-permalinks
> ```
>
> Note that the names of the executables have been updated, so you will need to update
> your `book.toml` as well:
>
> ```diff
> - [preprocessors.rustdoc-link]
> + [preprocessors.rustdoc-links]
> - [preprocessors.link-forever]
> + [preprocessors.permalinks]
> ```

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessors]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
<!-- prettier-ignore-end -->
