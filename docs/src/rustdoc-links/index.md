# mdbook-rustdoc-links

<div class="hidden">

**For best results, view this page at
<https://docs.tonywu.dev/mdbookkit/rustdoc-links>.**

</div>

Documenting your Rust project using [mdBook]? Link to your API docs à la rustdoc!

> Rustdoc is capable of directly linking to other rustdoc pages using the path of the
> item as a link. This is referred to as an ["intra-doc link"][intra-doc-link].

This [preprocessor] brings such _intra-doc links_ to mdBook. With it, you can
effortlessly add hyperlinks from your book to documentation hosted on
[docs.rs](https://docs.rs), as well as to Rust's
[Standard Library documentation](https://doc.rust-lang.org/stable/std).

Instead of finding and pasting URLs into your book, which can be tedious and
error-prone:

```md
The [`option`](https://doc.rust-lang.org/stable/std/option/index.html) and
[`result`](https://doc.rust-lang.org/stable/std/result/index.html) modules define
optional and error-handling types ...
```

You simply mention items by name, as if writing doc comments ...

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

Should you want to refactor your code, the preprocessor also checks your links, so you
can keep them up-to-date.

<figure>

{% include "/crates/mdbook-rustdoc-links/tests/book_homepage/stderr/data.svg" %}

</figure>

## Overview

Follow the [quickstart tutorial](getting-started.md) to try out the preprocessor!

For **writing documentation,**

- Learn about all the [syntax features for writing links](writing-links.md) when using
  this preprocessor. Hint: it's almost exactly the same as when writing doc comments.

- Learn how to [refer to Rust items by name](naming-items.md) in links so that the
  preprocessor can resolve them.

See how to configure the preprocessor **for specific scenarios,** such as:

- How to link to [items in dependencies](how-to/package-selection.md)

- How to link to [conditionally-compiled items](how-to/conditional-compilation.md)

- How to [use the preprocessor in CI/CD](how-to/continuous-integration.md)

Finally, you can check out the list of
[all available configuration options](configuration.md).

Happy linking!

> [!TIP]
>
> This preprocessor does _not_ require a nightly compiler to function.

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

[^1]: Text adapted from [<cite>A Tour of The Rust Standard Library</cite>][tour]

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[intra-doc-link]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[tour]: https://doc.rust-lang.org/stable/std/#a-tour-of-the-rust-standard-library
<!-- prettier-ignore-end -->
