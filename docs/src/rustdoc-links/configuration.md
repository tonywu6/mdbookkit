# Configuration

This page documents all the options that you can use to customize the preprocessor.

Each heading below corresponds to a configuration key. Unless otherwise specified, the
key is always under the `[preprocessor.rustdoc-links]` table.

## `[build]`

The `[build]` table customizes how the preprocessor
[compiles your API docs](naming-items.md#under-the-hood).

### `build.targets`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.targets = ["x86_64-unknown-linux-gnu"]
```

```toml config-example
[preprocessor.rustdoc-links.build]
targets = ["aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
```

</details></p>

- type: array of strings ([target triples])
- default: host platform

Build API docs for specific targets.

Useful for documenting platform-specific items. See the
[conditional compilation guide](how-to/conditional-compilation.md#specifying-targets)
for more details.

Should be an array of [target triples], such as `"x86_64-unknown-linux-gnu"`.

If unset, the preprocessor builds API docs for the current platform.

> [!IMPORTANT]
>
> When this option is used, the preprocessor generates docs.rs links that will include
> the specified target in their paths. Please see the relevant
> [section in the conditional compilation guide](how-to/conditional-compilation.md#caveat-target-aware-links)
> for the reasoning and implications of this behavior.

### `build.features`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.features = ["foo", "bar", "serde/derive"]
```

</details></p>

- type: array of strings
- default: none

Enable extra features when building API docs.

Useful for documenting feature-gated items. See the
[conditional compilation guide](how-to/conditional-compilation.md#specifying-features)
for more details.

> [!NOTE]
>
> {{#include _snippets/cargo-features-quirk.md}}

### `build.all-features`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.all-features = true
```

</details></p>

- type: boolean
- default: `false`

Pass the `--all-features` flag to `cargo doc` when building API docs.

See the
[conditional compilation guide](how-to/conditional-compilation.md#specifying-features)
for more details.

### `build.no-default-features`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.no-default-features = true
```

</details></p>

- type: boolean
- default: `false`

Pass the `--no-default-features` flag to `cargo doc` when building API docs.

See the
[conditional compilation guide](how-to/conditional-compilation.md#specifying-features)
for more details.

### `build.packages`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.packages = ["tap", "clap", "indexmap"]
# you can refer to these packages
```

```toml config-example
[preprocessor.rustdoc-links.build]
packages = [{ workspace = "all" }]
# you can refer to all packages in your workspace
```

```toml config-example
[preprocessor.rustdoc-links.build]
packages = [{ workspace = true, dependencies = true }]
# you can refer to all default members in your workspace,
# as well as their direct dependencies
```

</details></p>

- type: either an array of strings (package names) and/or selectors, or `"unspecified"`
- default: selects the default members of your workspace

Build docs only for specific packages. You can only create links to crates from packages
whose docs have been built.

Useful if you would like to refer to e.g. dependencies in your documentation. See the
[package selection guide](how-to/package-selection.md) for more details and possible
values.

If unset, the preprocessor only build docs for your workspace packages (specifically,
the [`default-members`] in your workspace).

### `build.preludes`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links.build]
preludes = [
  "serde::*",                # use serde::*;
  "url::Url",                # use url::Url;
  "std::io::{self, Result}", # use std::io::{self, Result};
]
```

</details></p>

- type: array of strings
- default: see below

Introduce additional items into scope when resolving links. **See
[Naming items](naming-items.md) for an introduction.**

Should be an array of paths that are valid to be placed in a [`use` declaration][`use`],
_without the leading `use` or the ending semicolon_.

Any item introduced through this option you may then refer to in your docs without
writing out their full paths. For example:

| Configuration                       | Effect                                                                                                    |
| :---------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `preludes = ["serde::*"]`           | [`[Serialize]`][serde::Serialize] will link to [`serde::Serialize`]                                       |
| `preludes = ["url::Url"]`           | [`[Url]`][url::Url] will link to [`url::Url`]                                                             |
| `preludes = ["std::io::{self, *}"]` | Both [`[io::Result]`][std::io::Result] and [`[Result]`][std::io::Result] will link to [`std::io::Result`] |

If unset, the default value of this option depends on your workspace layout:

- If you have a single package that is a library, then as a
  [convenience feature](naming-items.md#referring-to-your-own-crate), this option
  implicitly introduces every item exported from your library into scope, so that you
  don't have to repeat the crate name for every link.

- If your workspace has more than 1 default members, or if your package isn't a library,
  then this option by default does nothing.

### `build.cargo-args`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.cargo-args = ["--frozen"]
```

<figure>

```toml config-example
[preprocessor.rustdoc-links.build]
toolchain = "nightly"
cargo-args = [
  "-Z=rustdoc-map",
  "--config",
  "doc.extern-map.registries.crates-io='https://docs.rs/'",
]
```

<figcaption>

This example emulates how docs.rs handles broken links in API docs. See the
[self-hosting guide](how-to/self-hosting-cargo-docs.md#caveats) for more details.

</figcaption>

</figure>

</details></p>

- type: string or array of strings
- default: none

Extra command-line options to pass to all `cargo` invocations by the processor.

Note that because this option is used for all invocations, you should only use it for
options common to all `cargo` subcommands. For example, you may set the
[`--frozen` flag](https://doc.rust-lang.org/cargo/commands/cargo.html#manifest-options),
or enable unstable features through the
[`-Z` flag](https://doc.rust-lang.org/cargo/commands/cargo.html#option-cargo--Z).

Can be either an array of strings or a space-delimited string.

### `build.rustc-args`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.rustc-args = ["--check-cfg", "cfg(mdbook)", "--cfg", "mdbook"]
```

```toml config-example
[preprocessor.rustdoc-links.build]
rustc-args = "-C debug-assertions --verbose"
```

</details></p>

- type: string or array of strings
- default: none

Extra flags to pass to `rustc` when running `cargo doc` and `cargo check`. See the
[Cargo book](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags) for
more details.

Can be either an array of strings or a space-delimited string.

### `build.rustdoc-args`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.rustdoc-args = ["--test", "--sysroot", "/path/to/sysroot"]
```

</details></p>

- type: string or array of strings
- default: none

Extra flags to pass to `rustdoc` when running `cargo doc`. See the
[Cargo book](https://doc.rust-lang.org/cargo/reference/config.html#buildrustdocflags)
for more details.

Can be either an array of strings or a space-delimited string.

### `build.docs-rs`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.docs-rs = true
```

</details></p>

- type: boolean
- default: false

Inherit build options from your docs.rs configuration, which are defined in the
[`[package.metadata.docs.rs]` table](https://docs.rs/about/metadata) in your
`Cargo.toml` file.

This option is useful if you are already customizing your docs.rs builds, and you would
like to avoid repeating yourself. The preprocessor models most of its build options
after what docs.rs uses.

If you specify `build.docs-rs = true` but also specify individual options in
`book.toml`, options in `book.toml` take precedence, according to the following rules:

The following options are inherited unless they are set in `book.toml`:

- [`all-features`](#buildall-features)
- [`no-default-features`](#buildno-default-features)

The following array options from docs.rs are joined with the corresponding options in
`book.toml`, with array items in `book.toml` placed after those for docs.rs, having
higher precedence:

- [`features`](#buildfeatures)
- [`rustc-args`](#buildrustc-args)
- [`rustdoc-args`](#buildrustdoc-args)
- [`cargo-args`](#buildcargo-args)

The [`build.targets`](#buildtargets) will inherit from the combination of the following
docs.rs options _only if_ `build.targets` is not specified in `book.toml`; otherwise,
target-related options from docs.rs are ignored:

- `default-target`
- `targets`
- `additional-targets`

> [!NOTE]
>
> docs.rs doesn't support reading options from the workspace manifest (i.e. from a
> `[workspace.metadata.docs.rs]` table). For this reason, the preprocessor also does not
> support such usage.
>
> The preprocessor will report an error if it finds multiple packages that specify
> docs.rs options (which would cause ambiguity), or if there is no package that does.
> For this option to work, there must be exactly 1 package with that defines the
> `[package.metadata.docs.rs]` table after
> [filtering is done via the `build.packages` option](#buildpackages).

### `build.toolchain`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build.toolchain = "nightly"
```

</details></p>

- type: string
- default: none

Use a specific toolchain when running `cargo` and `rustdoc`.

The preprocessor will invoke subcommands with the
[`+toolchain` flag](https://doc.rust-lang.org/cargo/commands/cargo.html#option-cargo-+toolchain),
for example, `cargo +nightly doc`.

> [!NOTE]
>
> In other words, your toolchain must have been installed with rustup for this option to
> be correctly recognized.

## `[[build]]`

If you specify `preprocessor.rustdoc-link.build` as a TOML array instead of a table, you
enable the multi-stage build mode. In this mode, the preprocessor resolves links over
multiple "passes." **See the
[conditional compilation guide](how-to/conditional-compilation.md#multi-stage-builds)
for more details.**

Each item in the array should be a table. In each table, you can individually specify
any [`[build]` options documented above](#build).

Primarily useful if you need to document multiple packages and/or platforms, and they
have possibly conflicting build requirements. For example:

```toml config-example
[[preprocessor.rustdoc-links.build]]
targets = ["aarch64-unknown-linux-musl", "x86_64-unknown-linux-gnu"]
packages = [{ workspace = true }]

[[preprocessor.rustdoc-links.build]]
targets = ["x86_64-pc-windows-msvc"]
packages = ["windows-sys"]
features = ["Win32"]

[[preprocessor.rustdoc-links.build]]
targets = ["aarch64-apple-darwin"]
packages = ["security-framework"]
```

## `build-options`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
build-options = { toolchain = "nightly" }
build = [
  { targets = ["aarch64-unknown-linux-gnu"] },
  { targets = ["wasm32-wasip2"], no-default-features = true },
]
```

</details></p>

- type: table
- default: none

Use the `build-options` table to define shared options if you enable
[multi-stage builds](#build-1). Options in `build-options` will be merged into each item
in the `[[build]]` array.

- For options that take boolean or string values, those defined in `[[build]]` take
  precedence over those in `build-options`.

- For options that take arrays, except for [`build.targets`](#buildtargets), those
  defined in `build-options` are joined with those in each table in `[[build]]`, with
  the latter placed after the former.

- You cannot specify `targets` in `build-options`. Instead, specify it in each
  `[[build]]` table.

## `base-url`

<p><details>
  <summary>Example usage</summary>

<figure>

```toml config-example
[preprocessor.rustdoc-links]
base-url = "https://rustwasm.github.io/wasm-bindgen/api"
```

<figcaption>
  Link to an alternative site
</figcaption>

</figure>

<figure>

```toml config-example
[preprocessor.rustdoc-links]
base-url.dev = "/api"
```

<figcaption>
  Make API docs available through <code>localhost</code> during local development
</figcaption>

</figure>

<figure>

```toml config-example
[preprocessor.rustdoc-links]
base-url.release = "https://staging.docs.rs/{pkg_name}/{version}"
base-url.dev = "/api"
```

<figcaption>
  Use different base URLs for different environments
</figcaption>

</figure>

</details></p>

- type: string (URL or path); or [table](#base-urldev--base-urlrelease)
- default: `"https://docs.rs/{pkg_name}/{version}"`

Generate links with an alternative prefix.

By default, the preprocessor generates links that open in [docs.rs](https://docs.rs).
This option allows you to override this, if your API docs are hosted elsewhere.

Possible formats are:

[abcdef]

- a URL, such as `https://example.org`.
- a path, such as `/api`.

The URL or path may have the following placeholders, which will be filled in based on
the item being linked to:

- `{pkg_name}`, the name of the package being linked to
- `{version}`, the version of the package, _as defined in `Cargo.lock`_

Using a path for `base-url` has 2 main use cases. Please see the respective guides for
more details:

- [You would like to preview API docs during local development](how-to/local-development.md)
- [You would like to self-host your API docs](how-to/self-hosting-cargo-docs.md)

Links for [`std`] items always point to <https://doc.rust-lang.org> regardless of this
option.

### `base-url.dev` <br/> `base-url.release`

You can also use different URLs for different environments by setting `base-url.dev`
and/or `base-url.release` instead of just `base-url`, for example:

```toml config-example
[preprocessor.rustdoc-links]
# generate links to the GitHub Pages site when building in CI
base-url.release = "https://me.github.io/rust/docs"
# make docs previewable at `http://localhost:3000/api` when running locally
base-url.dev = "/api"
```

## `manifest-dir`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
manifest-dir = "../crates/library"
# if your book.toml is at `/path/to/book/book.toml`, then
# `manifest-dir` will be `/path/to/crates/library`
```

</details></p>

- type: string (a directory path[^relative-path])
- default: determined at runtime

If set, the preprocessor uses this path as the working directory when spawning `cargo`
commands.

Note that most of the time, you do not need to set this. As long as your book lives
anywhere within a Cargo workspace, the preprocessor will automatically determine the
workspace root at runtime.

## `fail-on-warnings`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.rustdoc-links]
fail-on-warnings = "always"
```

</details></p>

- type: string, either `"ci"` or `"always"`
- default: `"ci"`

Controls if the preprocessor will exit with a non-zero status code when there are
warnings during build. Warnings include log messages at the `WARN` level and diagnostics
at the `warning` level.

Can be either:

- `"ci"`. The preprocessor will fail if it is running in a CI environment and there are
  warnings.

  If it is not running in CI, the preprocessor prints out warnings but will exit with
  `0` (unless there are errors).

- `"always"`. The preprocessor will always fail as long as there are warnings.

The default is `"ci"`. This allows the preprocessor to fail your CI builds if it detects
potential issues, but keep `mdbook serve` running during local development.

Log messages and diagnostics at the error severity always cause the preprocessor to
fail, regardless of this option.

[^relative-path]:
    Relative paths are resolved relative to the directory that your `book.toml` file is
    in.

<!-- prettier-ignore-start -->
[target triples]: https://doc.rust-lang.org/stable/cargo/appendix/glossary.html#target
[`default-members`]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
[`use`]: https://doc.rust-lang.org/reference/items/use-declarations.html?highlight=use#use-declarations
<!-- prettier-ignore-end -->
