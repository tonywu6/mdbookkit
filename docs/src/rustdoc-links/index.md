# mdbook-rustdoc-links

<div class="hidden">

**For best results, view this page at
<https://docs.tonywu.dev/mdbookkit/rustdoc-links>.**

</div>

Link to Rust API docs by name in [mdBook], [_rustdoc_-style][rustdoc].

Instead of manually finding and pasting URLs, you simply write ...

```md
The [`option`][std::option] and [`result`][std::result] modules define optional and
error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter] module
defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to access
collections. [^1]
```

... and you get:

<figure class="fig-text">

The [`option`][std::option] and [`result`][std::result] modules define optional and
error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter] module
defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to access
collections. [^1]

</figure>

<figure>

![screen recording of mdbook-rustdoc-link during mdbook build](media/screencap.webp)

</figure>

> [!NOTE]
>
> This preprocessor depends on [rust-analyzer] to accurately resolve Rust items.

## Overview

Follow the [quickstart guide](getting-started.md) to try out the preprocessor.

For **writing documentation** —

- [Supported syntax](supported-syntax.md): Full list of link syntax with examples. Know
  how to link to additional items such as
  [functions, macros](supported-syntax.md#functions-and-macros), and
  [implementors](supported-syntax.md#fully-qualified-paths).

- [Name resolution](name-resolution.md): Understand how the preprocessor finds Rust
  items, including
  [when items are gated behind features](name-resolution.md#feature-gated-items).

For **making the preprocessor work with your project** —

- [Workspace layout](workspace-layout.md): Setup and options suitable for [Cargo
  workspaces][workspaces].

- [Caching](caching.md): If you are working on a large project and processing is taking
  a long time.

For **additional usage information** —

- [Standalone usage](standalone-usage.md): Use the preprocessor as a standalone command
  line tool.

- [Continuous integration](continuous-integration.md): Information for running the
  preprocessor in CI environments, including
  [logging](continuous-integration.md#logging) and
  [failing a build when there are bad links](continuous-integration.md#error-handling).

- [Configuration](configuration.md): List of available options.

- [Known issues](known-issues.md) and limitations.

Happy linking!

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

[^1]: Text adapted from [<cite>A Tour of The Rust Standard Library</cite>][tour]

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[rust-analyzer]: https://rust-analyzer.github.io/
[rustdoc]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[tour]: https://doc.rust-lang.org/stable/std/#a-tour-of-the-rust-standard-library
[workspaces]: https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html
<!-- prettier-ignore-end -->
