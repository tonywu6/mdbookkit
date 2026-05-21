# How to use the preprocessor in CI

You may use a continuous integration (CI) environment to build and deploy your book. For
example, you may use [GitHub Actions](https://docs.github.com/en/actions) to publish
your book to [GitHub Pages](https://docs.github.com/en/pages).

This guide discusses some specifics that may be helpful to know when using the
preprocessor in CI.

> [!TIP]
>
> See also the mdBook guide, which has a dedicated section about [using mdBook in CI in
> general][mdbook-ci].

## Things to know

Using the preprocessor in CI is largely the same as using it in local development.
However, some of the preprocessor's default behaviors are different when it
[detects](#detecting-ci) it is running in CI.

In most cases, no extra configurations are necessary for the preprocessor to work in CI.

### Exit status

When running locally, if the preprocessor detects non-fatal issues with your book, such
as broken links, it emits warnings but otherwise exits with a success (0) status. This
is so that mdBook will keep running even as there may be temporary errors as you are
editing your book.

When running in CI, any warnings emitted during the build process will cause the
preprocessor to exit with a failure (1) status at the end, in which case mdBook will
also exit early. This way, the preprocessor can fail your pipeline if you accidentally
pushed changes that contain problems.

<figure>

{% include "/crates/mdbook-rustdoc-links/tests/fail_on_warnings_in_ci/stderr/data.svg" %}

<figcaption>Example console output when the preprocessor has warnings in CI</figcaption>

</figure>

### Diagnostics

When running locally, the preprocessor prints diagnostic messages in a graphical style,
similar to how `rustc` prints them.

When running in CI, the preprocessor prints diagnostic messages as log messages:

<figure>

{% include "/crates/mdbook-rustdoc-links/tests/book_mdbookkit_term_logging/stderr/data.svg" %}

</figure>

You can override this behavior using the
[`MDBOOKKIT_TERM_GRAPHICAL`](../configuration/environment-variables.md#mdbookkit_term_graphical)
environment variable.

### Detecting CI

The preprocessor determines whether it's running in a CI environment by checking if the
`CI` environment variable is set. If the variable is set to any value except the empty
string, then the preprocessor will run in CI mode.

You usually don't need to configure this variable yourself, since most services should
automatically set it for you at runtime.

| Example value | CI mode? |
| :------------ | :------: |
| (unset)       |    no    |
| `CI=1`        |   yes    |
| `CI=true`     |   yes    |
| `CI=0`        |  _yes_   |
| `CI=false`    |  _yes_   |
| `CI=`         |    no    |

## Tips

This section lists some usual prerequisites for running the preprocessor in CI, as well
as some example pipeline configurations that you can use as starting points.

### Installing a Rust toolchain

Since the preprocessor [needs to run `cargo doc`](../naming-items.md#under-the-hood), a
Rust toolchain is required in the CI environment. For example, in GitHub Actions, you
can use the [`dtolnay/rust-toolchain`] action.

If you use the [`build.targets`](../configuration.md#buildtargets) option to generate
API docs for a fixed set of targets, then remember to also install the same targets in
CI.

### Installing the preprocessor

You can install a precompiled version of the preprocessor using [`cargo binstall`] (or
fetch it from [GitHub Releases][gh-releases]). As usual, you can also compile from
source by running `cargo install`.

For GitHub Actions, actions like [`taiki-e/install-action`] may be useful for installing
both mdBook and the preprocessor in one go.

## Example: GitHub Actions

```yaml
name: Build docs

on:
  release:
    types: [published]

jobs:
  build:
    name: Build
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v6

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install tools
        uses: taiki-e/install-action@v2
        with:
          tool: |
            mdbook
            mdbook-rustdoc-links
          fallback: cargo-binstall
          # the action does not officially support the preprocessor
          # as a tool, but it can use cargo-binstall as a fallback method

      - name: Build docs
        working-directory: docs
        run: |
          mdbook build

      - name: Upload docs
        uses: actions/upload-artifact@v7
        with:
          name: book
          path: docs/book
```

## Example: Tangled Spindles

TODO:

<!-- prettier-ignore-start -->
[`cargo binstall`]: https://github.com/cargo-bins/cargo-binstall
[`dtolnay/rust-toolchain`]: https://github.com/dtolnay/rust-toolchain
[`taiki-e/install-action`]: https://github.com/taiki-e/install-action/tree/v2/
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[mdbook-ci]: https://rust-lang.github.io/mdBook/continuous-integration.html
<!-- prettier-ignore-end -->
