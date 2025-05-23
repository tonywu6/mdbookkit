# Getting started

Follow these steps to start using `mdbook-rustdoc-link` in your book project!

## Install

You will need to:

1. Have [rust-analyzer]:

   - If you already use the [VS Code extension][ra-extension]: this crate automatically
     uses the server binary that comes with it, no extra setup is needed!
   - Otherwise, [install][ra-install] rust-analyzer (e.g. via `rustup`) and make sure
     it's on your `PATH`.

2. Install this crate:

   ```
   cargo install mdbookkit --features rustdoc-link
   ```

   Or you can grab precompiled binaries from [GitHub releases][gh-releases].

## Configure

Configure your `book.toml` to use it as a [preprocessor]:

```toml
[book]
title = "My Book"

[preprocessor.rustdoc-link]
# mdBook will run `mdbook-rustdoc-link`
after = ["links"]
# recommended, so that it can see content from {{#include}} as well
```

## Write

In your documentation, when you want to link to a Rust item, such as a type, a function,
etc., simply use its name in place of a URL, like this:

```md
Like [`std::thread::spawn`], [`tokio::task::spawn`] returns a
[`JoinHandle`][tokio::task::JoinHandle] struct.
```

The preprocessor will then turn them into hyperlinks:

<figure class="fig-text">

Like [`std::thread::spawn`], [`tokio::task::spawn`] returns a
[`JoinHandle`][tokio::task::JoinHandle] struct.

</figure>

This works in both `mdbook build` and `mdbook serve`!

![screen recording of mdbook-rustdoc-link during mdbook build](media/screencap.webp)

To read more about this project, feel free to return to [Overview](index.md#overview).

> [!IMPORTANT]
>
> It is assumed that you are running `mdbook` within a Cargo project.
>
> If you are working on a crate, and your book directory is within your source tree,
> such as next to `Cargo.toml`, then running `mdbook` from there will "just work".
>
> If your book doesn't belong to a Cargo project, refer to
> [Workspace layout](workspace-layout.md) for more information on how you can setup up
> the preprocessor.

> [!TIP]
>
> `mdbook-rustdoc-link` makes use of rust-analyzer's ["Open Docs"][open-docs] feature,
> which resolves links to documentation given a symbol.
>
> Items from `std` will generate links to <https://doc.rust-lang.org>, while items from
> third-party crates will generate links to <https://docs.rs>.
>
> So really, rust-analyzer is doing the heavy-lifting here. This crate is just the glue
> code :)

<!-- prettier-ignore-start -->

[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[open-docs]: https://rust-analyzer.github.io/book/features.html#open-docs
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[ra-extension]: https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer
[ra-install]: https://rust-analyzer.github.io/book/rust_analyzer_binary.html
[rust-analyzer]: https://rust-analyzer.github.io/

<!-- prettier-ignore-end -->
