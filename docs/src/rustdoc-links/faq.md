# Common/Known issues

## "failed to find a Cargo project"

This fatal error occurs because the preprocessor cannot find your Cargo project (package
or workspace), without which it cannot run
[`cargo doc`](naming-items.md#under-the-hood).

This could happen if the directory containing your book (containing the `book.toml`
file) is not within a Cargo project. You can tell the preprocessor where your project is
by setting the [`manifest-dir`](configuration.md#manifest-dir) option.

If you are not working with a Cargo project, then this preprocessor is not really
useful.

## "no item ... in module `temporary_crate_0`"

This warning could appear if you try to use the `crate::*` or `self::*` notation to link
to an item in your project, but you have not properly configured the preprocessor.

If your Cargo workspace consists of a single lib crate that you are documenting, then
the `crate::*` notation should work by default. Otherwise, this may not work out of the
box because:

1. You are working with a Cargo workspace containing multiple libraries, in which case
   the preprocessor wouldn't be able to tell which library "`crate`" refers to.
2. You are working with a bin crate.
3. You are trying to link to a private item.

In case you are have multiple libraries, you can use the
[`build.preludes`](naming-items.md#using-the-buildpreludes-option) option to explicitly
introduce items from your crate into the "`crate`" scope.

Note that the preprocessor currently does not support
[linking to private items](naming-items.md#items-that-cannot-be-linked-to).

## "cannot specify features for packages outside of workspace"

This fatal error could occur if you use both the
[`build.packages`](configuration.md#buildpackages) and the
[`build.features`](configuration.md#buildfeatures) option to enable features from
dependencies, like so:

```toml config-example
[preprocessor.rustdoc-links]
build.features = ["serde/derive"]
build.packages = ["serde"]
```

Due to a quirk in Cargo, selecting dependency features this way currently results in an
error. A workaround to include at least one workspace member in the `build.packages`
option, for example:

```diff config-example
  [preprocessor.rustdoc-links]
  build.features = ["serde/derive"]
- build.packages = ["serde"]
+ build.packages = [{ workspace = true }]
```

For more information, see
[Cargo issue #16990](https://github.com/rust-lang/cargo/issues/16990).

## "could not determine the versions of these packages"

This warning could occur if you try to link to a dev or build dependency by adding the
package to the [`build.packages`](configuration.md#buildpackages) option.

Due to an issue in Cargo, attempting to document dev or build dependencies results in
Cargo exiting with an error. Therefore, it is currently not possible to use the
preprocessor to generate links to them.

For more information, see
[Cargo issue #11105](https://github.com/rust-lang/cargo/issues/11105).

## "rustdoc did not process this link"

This warning diagnostic occurs when the preprocessor was not able to resolve a item but
`rustdoc` did not issue a diagnostic for it.

Normally, when an item fails to resolve, such as when the item does not exist, `rustdoc`
generates a corresponding warning. However, in very specific circumstances, `rustdoc`
considers an item resolved, but does not generate the necessary files for the
preprocessor to generate a URL. Some known examples are:

- If an item is marked as [`#[doc(hidden)]`][doc-hidden]. It is currently not possible
  to link to a hidden item.

- If an item from another crate is [re-exported with `#[doc(inline)]`][doc-inline], but
  that crate is not included in
  [the list of packages to build docs for](configuration.md#buildpackages). In this
  case, try rewriting the link to point to the original item instead of the re-exported
  location.

<!-- prettier-ignore-start -->
[doc-hidden]: https://doc.rust-lang.org/stable/rustdoc/write-documentation/the-doc-attribute.html#hidden
[doc-inline]: https://doc.rust-lang.org/stable/rustdoc/write-documentation/re-exports.html#inlining-with-docinline
<!-- prettier-ignore-end -->
