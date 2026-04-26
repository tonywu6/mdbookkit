# mdbook-rustdoc-links

<div class="hidden">

**For best results, view this page at
<https://docs.tonywu.dev/mdbookkit/rustdoc-links>.**

</div>

Link to Rust API docs by name in [mdBook], à la rustdoc!

> Rustdoc is capable of directly linking to other rustdoc pages using the path of the
> item as a link. This is referred to as an ["intra-doc link"][intra-doc-link].

This [preprocessor] brings such _intra-doc links_ to mdBook. With it, instead of
manually finding and pasting documentation URLs into your book:

```md
The [`option`](https://doc.rust-lang.org/stable/core/option/index.html) and
[`result`](https://doc.rust-lang.org/stable/core/result/index.html) modules define
optional and error-handling types ...
```

You simply write ...

```md
The [`option`][std::option] and [`result`][std::result] modules define optional and
error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter] module
defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to access
collections. [^1]
```

... and you will get:

<figure class="fig-text">

The [`option`][std::option] and [`result`][std::result] modules define optional and
error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter] module
defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to access
collections. [^1]

</figure>

## Overview

Follow the [quickstart tutorial](getting-started.md) to try out the preprocessor.

Happy linking!

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

[^1]: Text adapted from [<cite>A Tour of The Rust Standard Library</cite>][tour]

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[rust-analyzer]: https://rust-analyzer.github.io/
[intra-doc-link]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[tour]: https://doc.rust-lang.org/stable/std/#a-tour-of-the-rust-standard-library
[workspaces]: https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html
<!-- prettier-ignore-end -->
