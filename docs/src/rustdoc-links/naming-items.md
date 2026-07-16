# Naming items

When using intra-doc links in doc comments, it is possible to refer to anything [in
scope][in-scope], such as items defined in the same module, as well as `self`, `crate`,
etc. But how does this work when the links are in mdBook?

## Mental model

When using this preprocessor, you can _imagine your book as a standalone, empty crate._
By default, this "crate" also "depends on" your local package. If you are working with a
[Cargo workspace](how-to/cargo-workspaces.md), then it "depends on" all default members
in the workspace.

This means that, by default, you can always link to the following kinds of
items[^prelude]:

- Items from the prelude, for example:

  > ```md
  > [`u8`], [`assert!`], [`Option`], [`IntoFuture::Output`]
  > ```
  >
  > [`u8`], [`assert!`], [`Option`], [`IntoFuture::Output`]

- Items from `std`, by specifying their paths, for example:

  > ```md
  > - [`std::net::UdpSocket`]
  > - [`size_of::<T>()`][std::mem::size_of]
  > ```
  >
  > - [`std::net::UdpSocket`]
  > - [`size_of::<T>()`][std::mem::size_of]

- Items from crate(s) in your workspace, by specifying their paths, for example:

  > ```md
  > - [`mdbookkit::env::is_ci`]
  > - [`mdbookkit::markdown::patch_stream`]
  > ```
  >
  > - [`mdbookkit::env::is_ci`]
  > - [`mdbookkit::markdown::patch_stream`]

## Referring to your own crate

Of course, if you are documenting your own library, it would feel rather clumsy to have
to repeat the crate name for every link.

Therefore, as a convenience feature, if your workspace consists of a single library
crate[^default-member], then by default, everything publicly visible from that crate is
implicitly introduced into scope.

This means you can link to them without repeating the crate name, and use the `crate::*`
notation, as if you were writing a doc comment in `lib.rs`. Using this project as an
example, we have:

> ```md
> - [`env::is_ci`]
> - [`patch_stream`][crate::markdown::patch_stream]
> ```
>
> - [`env::is_ci`]
> - [`patch_stream`][crate::markdown::patch_stream]

## Using the `build.preludes` option

If you are documenting a workspace that features multiple libraries, then items from
them are not implicitly introduced, as that could create ambiguity.

Instead, to make things easier, you may use the
[`build.preludes`](reference/configuration.md#buildpreludes) configuration to explicitly
introduce items into scope.

As an example, assuming the crate `tracing_subscriber` is in your workspace. With the
following `book.toml`:

```toml config-example
[preprocessor.rustdoc-links]
build.preludes = ["tracing_subscriber::*"]
```

Items from that crate can now be linked to without having to write out the crate name:

> ```md
> [`EnvFilter`] implements the [`Layer`] trait.
> ```
>
> [`EnvFilter`] implements the [`Layer`] trait.

## Referring to dependencies

Of course, for more comprehensive documentation, you might wish to mention the APIs that
your libraries depend on as well.

The preprocessor provides the
[`build.packages`](reference/configuration.md#buildpackages) option that allows you to
build docs for extra packages. You may then refer to items in such packages using the
same syntax:

> ```md
> - [`Patch`][::annotate_snippets::Patch]
> - [`fmt`][fn@tracing_subscriber::fmt]
> ```
>
> - [`Patch`][::annotate_snippets::Patch]
> - [`fmt`][fn@tracing_subscriber::fmt]

To learn how to use the option, please see the dedicated
[How-to guide](how-to/package-selection.md).

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
  links may silently fail to appear.[^cargo-doc-quirk]

## Under the hood

The preprocessor receives the full text of your book content from mdBook, from which it
collects all the links that look like Rust item and need to be converted.

**The preprocessor then runs `cargo doc` in your Cargo workspace.** Without going into
too much details, running `cargo doc` prepares the necessary files, such as compiler
artifacts and HTML files, so that items can be _correctly_ resolved in the next steps.

The preprocessor then **synthesizes a Rust snippet** containing the links it collected
from your book, which would look something like this:

```rs
//! - [`IntoFuture::Output`]
//! - [`std::net::UdpSocket`]
//! - [`option`][std::option]
//! - [The `fmt` module][mod@fmt]
use tracing_subscriber::*; // from `build.preludes`
```

(This is effectively a temporary `lib.rs` file, hence the concept of a
["standalone, empty crate"](#mental-model)!)

The preprocessor then **invokes [`rustdoc`] with this snippet,** alongside things such
as the artifacts produced by `cargo doc`. If written as a shell command, this would look
something like:

```sh
rustdoc --out-dir "/tmp/temporary_crate_0/doc" --crate-type lib --error-format json \
  --extern "tracing_subscriber=/target/debug/deps/libtracing_subscriber.rmeta" \
  --extern "awesome_crate=/target/debug/deps/libawesome_crate.rmeta" \
  -L "dependency=/target/debug/deps" \
  /tmp/temporary_crate_0/src/lib.rs
```

> [!TIP]
>
> If you are curious, try running `mdbook serve` with the environment variable
> `MDBOOK_LOG` set to `info,mdbook_rustdoc_links=debug`, for example:
>
> ```sh
> MDBOOK_LOG=info,mdbook_rustdoc_links=debug mdbook serve
> ```
>
> The preprocessor will print out extra information, including the command-line
> arguments that it supplies to `cargo doc` and `rustdoc`.

The preprocessor then parses the HTML output produced by `rustdoc`, which would include
something like:

<!-- prettier-ignore-start -->
```html
<li><a href="https://doc.rust-lang.org/1.95.0/core/future/into_future/trait.IntoFuture.html#associatedtype.Output">
  <code>IntoFuture::Output</code>
</a></li>
<li><a href="../tracing_subscriber/fmt/index.html">
  The <code>fmt</code> module
</a></li>
```
<!-- prettier-ignore-end -->

From here, the preprocessor will then have enough information to construct the full URL
of each link that needs to be resolved, and finally return them to mdBook to continue
the build process. Voila!

[^prelude]:
    To be more precise, the preprocessor is currently
    [hard-coded to use the 2024 edition prelude](/crates/mdbook-rustdoc-links/src/builder.rs#L230),
    and `std` is always available.

[^default-member]:
    This also applies if your workspace has multiple crates, but only one library crate
    as the [default member][default-member].

[^cargo-doc-quirk]:
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
[`rustdoc`]: https://doc.rust-lang.org/rustdoc
<!-- prettier-ignore-end -->
