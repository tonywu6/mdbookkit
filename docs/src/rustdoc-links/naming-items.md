# Naming items

When using intra-doc links in doc comments, it is possible to refer to anything [in
scope][in-scope], such as items defined in the same module, as well as `self`, `crate`,
etc. But how does this work when the links are in mdBook?

## Mental model

When using this preprocessor, you may _imagine your book as a standalone, empty crate._
By default, this "crate" also "depends on" the package(s) in your workspace, as well as
their dependencies.

This means that, by default, you can always link to the following kinds of items[^1]:

- From the prelude, for example:

  > ```md
  > [`u8`], [`assert!`], [`Option`], [`IntoFuture::Output`]
  > ```
  >
  > [`u8`], [`assert!`], [`Option`], [`IntoFuture::Output`]

- From `std`, by specifying their paths, for example:

  > ```md
  > - [`std::net::UdpSocket`]
  > - [`size_of::<T>()`][std::mem::size_of]
  > ```
  >
  > - [`std::net::UdpSocket`]
  > - [`size_of::<T>()`][std::mem::size_of]

- From any crates in the workspace, including local crates and external dependencies, by
  specifying their paths, for example:

  > ```md
  > - [`mdbookkit::env::is_ci`]
  > - [`Patch`][::annotate_snippets::Patch]
  > - [`fmt`][fn@tracing_subscriber::fmt]
  > ```
  >
  > - [`mdbookkit::env::is_ci`]
  > - [`Patch`][::annotate_snippets::Patch]
  > - [`fmt`][fn@tracing_subscriber::fmt]

  > [!TIP]
  >
  > In other words, anything documentable by `cargo doc` can be linked to this way.

## Linking to your own crate

Of course, if you are documenting your own library, it would feel rather clumsy to have
to repeat the crate name for every link.

Therefore, as a convenience feature, if your workspace consists of a single library
crate[^2], then by default, everything publicly visible from that crate is implicitly
introduced into scope.

This means you can link to them without repeating the crate name, or use the `crate::*`
notation, as if you were writing a doc comment in `lib.rs`. Using this project as an
example, we have:

> ```md
> - [`env::is_ci`]
> - [`PatchStream`][crate::markdown::PatchStream]
> ```
>
> - [`env::is_ci`]
> - [`PatchStream`][crate::markdown::PatchStream]

## Using the `build.preludes` option

If you are documenting a workspace that features multiple libraries, then their exported
items are not implicitly introduced, as that could create ambiguity.

Instead, to make things easier, you may use the `build.preludes` configuration to
explicitly introduce items into scope.

As an example, with the following `book.toml`:

```toml config-example-rustdoc-links
[preprocessor.rustdoc-links]
build.preludes = ["tracing_subscriber::*"]
```

Items from the [`tracing_subscriber`] crate can now be linked to without writing out the
crate name:

> ```md
> [`EnvFilter`] implements the [`Layer`] trait.
> ```
>
> [`EnvFilter`] implements the [`Layer`] trait.

## Items that cannot be linked to

Due to some limitations, it is not possible to create links to the following kinds of
items:

- **Private items.** Even though `rustdoc` is capable of [documenting private
  items][document-private-items], items must still be publicly reachable for this
  preprocessor to correctly generate links.

- **Items from binary crates.** Likewise, because items in a `bin` crate cannot be
  public, it is not possible to link to them.

- **Hidden items.** Linking to hidden items will likely fail, unless the item is
  [re-exported]. This is due to a [quirk in `rustdoc`][rust-issue-81979] where intra-doc
  links may silently fail to appear.[^3]

[^1]:
    To be more precise, the preprocessor is currently
    [hard-coded to use the 2024 edition prelude](/crates/mdbook-rustdoc-links/src/builder.rs#L230),
    and `std` is always available.

[^2]:
    This also applies if your workspace has multiple crates, but only one library crate
    as the [default member][default-member].

[^3]:
    The quirk is that `rustdoc` will not generate a link if it can't find a
    corresponding HTML page at the destination, and `rustdoc` does not generate HTML
    pages for hidden items unless their documentation are inlined elsewhere. Although
    there is a `--document-hidden-items` toggle, this is not currently provided as an
    option in this preprocessor.

<!-- prettier-ignore-start -->
[in-scope]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html#valid-links
[default-member]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
[document-private-items]: https://doc.rust-lang.org/rustdoc/command-line-arguments.html#--document-private-items-show-items-that-are-not-public
[re-exported]: https://doc.rust-lang.org/rustdoc/write-documentation/re-exports.html
[rust-issue-81979]: https://github.com/rust-lang/rust/issues/81979
<!-- prettier-ignore-end -->
