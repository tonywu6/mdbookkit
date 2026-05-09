# How to choose the packages to build docs for

Without explicit configuration, the preprocessor will always build documentation for all
packages and dependencies in your workspace by running `cargo doc` without specifying
the packages to build. If the workspace contains many packages, this could take a very
long time.

> [!NOTE]
>
> This is true even if your book is within a member package! This is because the
> preprocessor always runs `cargo doc` from your workspace's root directory (where the
> root `Cargo.toml` is). This differs from [how `cargo doc` normally
> behaves][cargo-doc-package-selection].

If you are only ever documenting a few of the packages in your book, then you can use
the `build.packages` option to explicitly specify the packages to run `cargo doc` on.

You can specify the names of the packages to build. This includes direct and transitive
dependencies.[^cargo-doc-dev-deps] Note that names should be **package names, _not_
crate names.**

```toml config-example
[preprocessor.rustdoc-links]
build.packages = ["serde_json", "tracing-subscriber"]
```

When the `build.packages` option has been specified, docs for your **local packages are
<br> _no longer_ built by default.**

Specify `{ workspace = true }` to build docs for all [**default** workspace
members][default-member]:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [{ workspace = true }]
```

Specify `{ workspace = "all" }` to build docs for **all members** rather than just
default members:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [{ workspace = "all" }]
```

Specify `{ dependencies = true }` to build docs for the package itself _and_ its
_direct_ dependencies.

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [
  { name = "serde_json", dependencies = true }
]
# you can now refer to `serde_json` and `serde_core` in docs
```

When paired with `{ workspace = ... }`, the preprocessor will build docs for every
(default) workspace member _and_ their direct dependencies:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [
  { workspace = true, dependencies = true }
]
```

Of course, these can be used together:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [
  "camino",
  { name = "lol_html", dependencies = true },
  { workspace = "all" },
]
```

[^cargo-doc-dev-deps]:
    Due to a [quirk in `cargo doc`][cargo-doc-dev-deps], it is currently not possible to
    specify `dev-dependencies` or `build-dependencies` this way.

<!-- prettier-ignore-start -->
[cargo-doc-dev-deps]: https://github.com/rust-lang/cargo/issues/11105
[cargo-doc-package-selection]: https://doc.rust-lang.org/cargo/commands/cargo-doc.html#package-selection
[default-member]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
<!-- prettier-ignore-end -->
