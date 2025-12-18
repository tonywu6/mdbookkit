# Getting started

## Install

```
cargo install mdbook-permalinks
```

Alternatively, you may obtain precompiled binaries from [GitHub releases][gh-releases].

## Configure

Configure your `book.toml` to use the installed program as a [preprocessor]:

```toml
[book]
title = "My Book"

[output.html]
git-repository-url = "https://github.com/me/my-awesome-crate"

[preprocessor.permalinks]
```

- The `git-repository-url` option in the `[output.html]` table controls the [icon link
  that appears in the menu bar in the top-right corner][git-repository-url]. If
  configured, the preprocessor will reuse this link as the base URL for the generated
  permalinks.

- The `[preprocessor.permalinks]` table enables the preprocessor. mdBook will execute
  the command `mdbook-permalinks`.

## Write

Link to files in your Git repository using the relative paths from your Markdown source
files to the files to link. For example:

```md
See [`book.toml`](../../book.toml) for an example config.
```

<figure class="fig-text">

See [`book.toml`](../../book.toml) for an example config.

</figure>

## Next steps

- See the list of [features](features.md) in detail.
- Check out [available configuration options](configuration.md).
- Learn about [known issues and limitations](known-issues.md).

<!-- prettier-ignore-start -->

[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[git-repository-url]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html

<!-- prettier-ignore-end -->
