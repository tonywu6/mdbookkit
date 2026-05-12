# How to create links to additional packages

Without explicit configuration, the preprocessor will only build documentation for your
local packages, but not your dependencies. If you would like to refer to more than that,
then you can use the `build.packages` option to explicitly specify the packages to run
`cargo doc` on.

The option should be specified as an array. You can explicitly specify the names of the
packages:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = ["serde_json", "tracing-subscriber"]
```

Both direct and transitive dependencies are supported[^cargo-doc-dev-deps], as are local
packages. Note that names should be **package names, _not_ crate names.**

> [!IMPORTANT]
>
> If the `build.packages` option is specified, then docs for your **local packages are
> <br> _no longer_ built by default.** You can add them back using the following special
> syntax.

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
[default-member]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
<!-- prettier-ignore-end -->
