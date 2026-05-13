# Getting started

This tutorial will walk you through the steps necessary to use the preprocessor. After
which, you will be writing some links in an mdBook project to see how the preprocessor
works.

## Prerequisites

This tutorial assumes that:

- You already have a working mdBook project. If you need to, feel free to follow
  [mdBook's tutorial][mdbook-tutorial] to first create a book.

- You already have a Cargo [project][cargo-project], and your book is in the project's
  directory.

  <details>
    <summary>Explanation</summary>

  Under the hood, the preprocessor runs [`cargo doc`] so that it can correctly generate
  links, which requires the presence of a Cargo project. Outside of a Cargo project,
  this preprocessor isn't really useful.

  If you aren't currently working on a package, for this tutorial, the quickest way to
  get started is to run [`cargo init`] in your book's top-level directory (where the
  `book.toml` file is).

  </details>

## Install

A ["preprocessor"] is just an executable that mdBook will run during builds to customize
the build process. You can build and install this preprocessor from source using
`cargo`:

```sh
cargo install mdbook-rustdoc-links
```

<details>
  <summary>Other ways to install</summary>

- This project supports [cargo-binstall], so instead of compiling from source, you can
  install a precompiled binary:

  ```sh
  cargo binstall mdbook-rustdoc-links
  ```

- You can also download binaries directly from [GitHub releases][gh-releases].

</details>

## Configure

In your `book.toml`, add the following options to enable the preprocessor:

```toml config-example
[preprocessor.rustdoc-links]
after = ["links"]
```

<details>
  <summary>Explanation</summary>

```diff config-example
+ [preprocessor.rustdoc-links]
```

Adding this table tells mdBook to execute the command `mdbook-rustdoc-links` during
builds.

```diff config-example
  [preprocessor.rustdoc-links]
+ after = ["links"]
```

Adding this tells mdBook to run this preprocessor after the default [`links`
preprocessor][mdbook-links]. This is recommended because it allows the preprocessor to
see text embedded using the [`{{#include}}` directive][mdbook-include].

</details>

## Write

You are now ready to add intra-doc links to your book.

For this tutorial, add the following Markdown paragraph to any page:

```md
A type implementing [`Sized`] has a constant size known at compile time.
```

In this example, ``[`Sized`]`` is the [intra-doc link][intra-doc-link]. If you've used
`cargo doc` before, then this notation may already look familiar to you. You may also
have seen this type of notation in [doc comments][doc-comment] in Rust source code.

> [!TIP]
>
> If you are not yet familiar with how documenting Rust code works, feel free to review
> the relevant chapter in [the book][publishing-to-crates-io] first!

You may now run `mdbook serve`. In the rendered page, you should see the following text
containing the desired link:

<figure class="fig-text">

A type implementing [`Sized`] has a constant size known at compile time.

</figure>

Feel free to keep `mdbook serve` running and add more items to the document, and see how
they are converted to links! Here are some example sentences:

```md
- The first collection type we'll look at is [`Vec<T>`], also known as a vector.
- We'll need the [`std::env::args`] function provided in Rust's standard library.
- To create a new thread, we call the [`thread::spawn`][std::thread::spawn] function and
  pass it a closure.
```

<details>
  <summary>Here's how they would look like when rendered</summary>

- The first collection type we'll look at is [`Vec<T>`], also known as a vector.
- We'll need the [`std::env::args`] function provided in Rust's standard library.
- To create a new thread, we call the [`thread::spawn`][std::thread::spawn] function and
  pass it a closure.

</details>

## Check

But what happens if an item cannot be resolved? For example,

- You may have made a typo when naming the item; or
- The item you previously linked to has moved during an incompatible update, and you
  haven't updated your book yet.

Because this preprocessor utilizes rustdoc, you also get rustdoc's [linting
support][rustdoc-lints] out of the box!

For this tutorial, let's say you made a typo, and wrote ``[`Size`]`` instead of
``[`Sized`]``:

```diff
- A type implementing [`Sized`] has a constant size known at compile time.
+ A type implementing [`Size`] has a constant size known at compile time.
```

Edit the paragraph and save. If `mdbook serve` is still running, you should now see a
warning in your terminal:

<figure>

{{#include ../../../crates/mdbook-rustdoc-links/tests/book_getting_started/stderr/data.svg}}

<figcaption>

Formatting of diagnostics powered by [annotate-snippets][annotate_snippets]

</figcaption>

</figure>

## Next steps

<!-- prettier-ignore-start -->
[`cargo doc`]: https://doc.rust-lang.org/cargo/commands/cargo-doc.html
[`cargo init`]: https://doc.rust-lang.org/cargo/commands/cargo-init.html
[cargo-binstall]: https://github.com/cargo-bins/cargo-binstall
[cargo-project]: https://doc.rust-lang.org/cargo/guide/why-cargo-exists.html
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[intra-doc-link]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[mdbook-include]: https://rust-lang.github.io/mdBook/format/mdbook.html#including-files
[mdbook-links]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html#:~:text=The%20following%20preprocessors%20are%20built%2Din%20and%20included%20by%20default:
[mdbook-tutorial]: https://rust-lang.github.io/mdBook/guide/creating.html
["preprocessor"]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[rustdoc-lints]: https://doc.rust-lang.org/rustdoc/lints.html
[doc-comment]: https://doc.rust-lang.org/reference/comments.html#doc-comments
[publishing-to-crates-io]: https://doc.rust-lang.org/book/ch14-02-publishing-to-crates-io.html
<!-- prettier-ignore-end -->
