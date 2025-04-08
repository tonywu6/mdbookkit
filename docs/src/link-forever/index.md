# mdbook-link-forever

mdBook [preprocessor] that takes care of linking to files in your Git repository.

`mdbook-link-forever` rewrites path-based links to version-pinned GitHub permalinks. No
more hard-coded GitHub URLs.

```md
Here's a link to the [Cargo workspace manifest](../../../Cargo.toml).
```

<figure class="fig-text">

Here's a link to the [Cargo workspace manifest](../../../Cargo.toml).

</figure>

- Versions are determined at build time. Supports both tags and commit hashes.
- Because paths are readily accessible at build time, it also
  [validates](features.md#link-validation) them for you.

## Getting started

1. Install this crate:

   ```
   cargo install mdbookkit --features link-forever
   ```

2. Configure your `book.toml`:

   ```toml
   [book]
   title = "My Book"

   [output.html]
   git-repository-url = "https://github.com/me/my-awesome-crate"
   # will use this for permalinks

   [preprocessor.link-forever]
   # mdBook will run `mdbook-link-forever`
   ```

3. Link to files using paths, like this:

   ```md
   See [`book.toml`](../../book.toml#L44-L48) for an example config.
   ```

   <figure class="fig-text">

   See [`book.toml`](../../book.toml#L44-L48) for an example config.

   </figure>

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

<!-- prettier-ignore-start -->

[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html

<!-- prettier-ignore-end -->
