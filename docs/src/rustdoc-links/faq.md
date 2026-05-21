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
