# mdbook-permalinks

Create permalinks to files in your Git repository using paths.

Link to source code, examples, configuration files, etc., in your [mdBook]
documentation, without having to hard-code URLs or worry about broken links. You simply
write ...

```md
Here is a link to the project's [Cargo.toml](../../../Cargo.toml).
```

... and you get:

<figure class="fig-text">

Here is a link to the project's [Cargo.toml](../../../Cargo.toml).

</figure>

## Overview

- Create [permalinks](features.md#permalinks) by file path.
- Links are [pinned to the tag or commit](features.md#versioning) that is checked out at
  build time.
- Links are [validated](features.md#link-validation) during build. Receive warnings when
  a linked file become missing.
- Repository URLs are [autoconfigured for GitHub](features.md#repo-url-auto-discovery),
  or you can use a [custom URL scheme](configuration.md#repo-url-template).

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
<!-- prettier-ignore-end -->
