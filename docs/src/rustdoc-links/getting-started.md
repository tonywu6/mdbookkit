# Getting started

This tutorial will walk you through the setup necessary to use this preprocessor and be
able to use rustdoc-style intra-doc links in your mdBook project.

## Prerequisites

This tutorial assumes that:

- You already have a working book. If you need to, feel free to follow [mdBook's
  tutorial][mdbook-tutorial] to first create a book.

- You already have a Cargo [project][cargo-project], and your book is in the project's
  directory.

  <details>
    <summary>Explanation</summary>

  Under the hood, the preprocessor runs [`cargo doc`] to be able to correctly generate
  links, which requires the presence of a Cargo project. Outside of a Cargo project,
  this preprocessor isn't really useful.

  If you aren't currently working on a package, for this tutorial, the quickest way to
  get started is to run [`cargo init`] in your book's top-level directory (where the
  `book.toml` file is).

  </details>

## Install

A [preprocessor] is just an executable that mdBook will run during builds to customize
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

In your `book.toml`, add the following options:

```toml config-example-rustdoc-links
[preprocessor.rustdoc-links]
after = ["links"]
```

<details>
  <summary>Explanation</summary>

```diff
+ [preprocessor.rustdoc-links]
```

Adding this table tells mdBook to execute the command `mdbook-rustdoc-links` during
builds.

```diff
  [preprocessor.rustdoc-links]
+ after = ["links"]
```

Adding this tells mdBook to run this preprocessor after the default [`links`
preprocessor][mdbook-links]. This is recommended because it allows the preprocessor to
see text embedded using the [`{{#include}}` directive][mdbook-include].

</details>

## Write

You are now ready to use [intra-doc links][intra-doc-link] in your book.

For this tutorial, add the following Markdown paragraph to any page:

```md
A type implementing [`Sized`] has a constant size known at compile time.
```

In this example, ``[`Sized`]`` is the intra-doc link. If you had used `cargo doc`
before, then this syntax likely looks familiar, because this is one of the link syntax
that `cargo doc` (i.e. rustdoc) supports.

You may now run `mdbook serve`. In the rendered page, you should see the following text
containing the desired link:

<figure class="fig-text">

A type implementing [`Sized`] has a constant size known at compile time.

</figure>

This preprocessor is not magic. During the build process, it gathers all the links in
your book that may need conversion, then simply calls out to `cargo doc` to do the
actual link resolution work.

This means that any item that you can put in [doc comments][doc-comment] that can be
rendered successfully by `cargo doc`, you can put in your book. This includes links to
your own packages, to [`std`] and [`core`], and to your packages' dependencies (and even
their transitive dependencies!)

But what if an item cannot be linked? For example,

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
[doc-comment]: https://doc.rust-lang.org/reference/comments.html#doc-comments
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[intra-doc-link]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[mdbook-include]: https://rust-lang.github.io/mdBook/format/mdbook.html#including-files
[mdbook-links]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html#:~:text=The%20following%20preprocessors%20are%20built%2Din%20and%20included%20by%20default:
[mdbook-tutorial]: https://rust-lang.github.io/mdBook/guide/creating.html
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[rustdoc-lints]: https://doc.rust-lang.org/rustdoc/lints.html
<!-- prettier-ignore-end -->
