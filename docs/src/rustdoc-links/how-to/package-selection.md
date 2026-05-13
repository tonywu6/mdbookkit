# How to create links to additional packages

As explained in [Naming items](../naming-items.md#under-the-hood), the preprocessor
needs to run `cargo doc` to be able to resolve links. You can only create links to
crates whose packages `cargo doc` has documented.

By default, only your local packages are documented. If you would like to refer to items
in other packages, then you can use the `build.packages` option to explicitly specify
more packages to run `cargo doc` on.

## Specifying packages by name

You can explicitly specify the names of the packages to document:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = ["serde_json", "tracing-subscriber"]
```

Both direct and transitive dependencies are supported[^cargo-doc-dev-deps], as are local
packages. Note that names should be **package names, _not_ crate names.**

> [!IMPORTANT]
>
> If the `build.packages` option is specified, then docs for your **workspace packages
> are <br> _no longer_ built by default.** You can add them back using the following
> special syntax.

## Documenting workspace packages

Specify `{ workspace = true }` to build docs for [_default_ workspace
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

## Documenting dependencies

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

> [!TIP]
>
> The preprocessor learns about package dependencies by running:
>
> ```sh
> cargo tree --depth 1 --edges normal
> ```

Of course, the above syntax can be used together:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = [
  "camino",
  { name = "lol_html", dependencies = true },
  { workspace = "all" },
]
```

## Documenting everything

Finally, you can write `packages = "unspecified"`:

```toml config-example
[preprocessor.rustdoc-links]
build.packages = "unspecified"
```

This causes the preprocessor to run `cargo doc` without naming any package (that is,
without using the [`--package` flag][cargo-doc-package]), which will document all
default workspace members and all their dependencies, direct and transitive. As a
result, you can refer to items in all these packages.

Note that if your project has many dependencies, the first build could take a long time!

[^cargo-doc-dev-deps]:
    It is currently not possible to specify `dev-dependencies` or `build-dependencies`
    this way. See [cargo issue#11105] for more details.

<!-- prettier-ignore-start -->
[cargo issue#11105]: https://github.com/rust-lang/cargo/issues/11105
[default-member]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
[cargo-doc-package]: https://doc.rust-lang.org/cargo/commands/cargo-doc.html#option-cargo-doc---package
<!-- prettier-ignore-end -->
