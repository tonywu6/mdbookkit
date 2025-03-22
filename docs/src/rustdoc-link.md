# mdbook-rustdoc-link

**[_rustdoc_-style linking][rustdoc] for [mdBook]** (with the help of [rust-analyzer]).

> _You write:_
>
> ```md
> The [`option`][std::option] and [`result`][std::result] modules define optional and
> error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter]
> module defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to
> access collections. [^1]
> ```
>
> _You get:_
>
> The [`option`][std::option] and [`result`][std::result] modules define optional and
> error-handling types, [`Option<T>`] and [`Result<T, E>`]. The [`iter`][std::iter]
> module defines Rust's iterator trait, [`Iterator`], which works with the `for` loop to
> access collections. [^1]

## Getting started

`mdbook-rustdoc-link` is an mdBook [preprocessor]. First, install it:

```
cargo install mdbook-rustdoc-link
```

Next, configure your `book.toml`:

```toml
[book]
title = "My Book"
# other configuration

[preprocessor.rustdoc-link]
# ^ mdBook will run `mdbook-rustdoc-link`
after = ["links"]
# ^ recommended, so that it can see content from {{#include}} as well
```

## Problem statement

---

[^1]: Text adapted from [<cite>A Tour of The Rust Standard Library</cite>][tour]

[rustdoc]:
  https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[preprocessor]:
  https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[rust-analyzer]: https://rust-analyzer.github.io/
[mdBook]: https://rust-lang.github.io/mdBook/
[tour]: https://doc.rust-lang.org/stable/std/#a-tour-of-the-rust-standard-library
