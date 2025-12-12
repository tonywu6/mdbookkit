# Workspace layout

As mentioned in [Name resolution](name-resolution.md), the preprocessor must know where
your crate's entrypoint is.

To do that, it will try to locate the nearest `Cargo.toml` by running
[`cargo locate-project`][locate-project], by default from the current working directory.

If you have a single-package setup, this should "just work", regardless of where your
book directory is within your source tree.

If you are using [Cargo workspaces][workspaces], then the preprocessor may fail with the
message:

```
Error: Cargo.toml does not have any lib or bin target
```

In this case, the preprocessor has located the workspace `Cargo.toml` rather than that
of a specific package, and some extra setup is required.

## Using the `manifest-dir` option

In your `book.toml`, in the `[preprocessor.rustdoc-link]` table, set the
[`manifest-dir`](configuration.md#manifest-dir) option to the relative path to a member
package.

For example, if you have the following workspace layout:

```
my-workspace/
├── crates/
│   └── fancy-crate/
│       ├── src/
│       │   └── lib.rs
│       └── Cargo.toml
└── docs/
    ├── src/
    │   └── SUMMARY.md
    └── book.toml
```

Then in your `book.toml`:

```toml
[preprocessor.rustdoc-link]
manifest-dir = "../crates/fancy-crate"
```

> [!IMPORTANT]
>
> `manifest-dir` should be a path **relative to `book.toml`**, not relative to workspace
> root.

## Placing your book inside a member package

If you have a "main" package, you can also move your book directory to that package, and
run mdBook from there:

```
my-workspace/
└── crates/
    ├── fancy-crate/
    │   ├── docs/
    │   │   ├── src/
    │   │   │   └── SUMMARY.md
    │   │   └── book.toml
    │   ├── src/
    │   │   └── lib.rs
    │   └── Cargo.toml
    └── util-crate/
        └── ...
```

## Documenting multiple packages

If you would like to document items from several independent packages, but still would
like to centralize your book in one place — unfortunately, the preprocessor does not yet
have the ability to work with multiple entrypoints.

A possible workaround would be to turn your book folder into a private package that
depends on the packages you would like to document. Then you can link to them as if they
were third-party packages. The generated links will point to the original packages
instead of this private package.

```
my-workspace/
├── crates/
│   ├── fancy-crate/
│   │   └── Cargo.toml
│   └── awesome-crate/
│       └── Cargo.toml
├── docs/
│   ├── Cargo.toml
│   └── book.toml
└── Cargo.toml
```

```toml
# docs/Cargo.toml
[dependencies]
fancy-crate = { path = "../crates/fancy-crate" }
awesome-crate = { path = "../crates/awesome-crate" }
```

```toml
# Cargo.toml
[workspace]
members = ["crates/*", "docs"]
default-members = ["crates/*"]
resolver = "2"
```

## Using without a Cargo project

If your book isn't for a Rust project, but you still find a use in this preprocessor
(e.g. perhaps you would like to mention `std`) — unfortunately, the preprocessor does
not yet support running without a Cargo project.

Instead, you can setup your book project as a private, dummy package.

```
my-book/
├── src/
│   └── SUMMARY.md
├── book.toml
└── Cargo.toml
```

```toml
# Cargo.toml
[dependencies]
# empty, or you can add anything you need to document
```

<!-- prettier-ignore-start -->

[locate-project]: https://doc.rust-lang.org/cargo/commands/cargo-locate-project.html
[workspaces]: https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html

<!-- prettier-ignore-end -->
