# How to use the preprocessor in CI

{% include "/docs/src/_snippets/continuous-integration/preface.md" %}

## Things to know

<!-- prettier-ignore-start -->
{% with
  exit_status = "/crates/mdbook-rustdoc-links/tests/fail_on_warnings_in_ci/stderr/data.svg",
  log_messages = "/crates/mdbook-rustdoc-links/tests/book_mdbookkit_term_logging/stderr/data.svg"
%}
  {% include "/docs/src/_snippets/continuous-integration/things-to-know.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

## Tips

This section lists some usual prerequisites for running the preprocessor in CI, as well
as some example pipeline configurations that you can use as starting points.

### Installing a Rust toolchain

Since the preprocessor [needs to run `cargo doc`](../naming-items.md#under-the-hood), a
Rust toolchain is required in the CI environment. For GitHub Actions, for example, you
can use the [`dtolnay/rust-toolchain`] action.

If you use the [`build.targets`](../configuration.md#buildtargets) option to generate
API docs for a fixed set of targets, then remember to also install the same targets in
CI.

### Installing the preprocessor

You can install a precompiled version of the preprocessor using [`cargo binstall`], or
fetch it from [GitHub Releases][gh-releases]. As usual, you can also compile from source
by running `cargo install`.

For GitHub Actions, actions like [`taiki-e/install-action`] may be useful for installing
both mdBook and the preprocessor in one go.

> [!TIP]
>
> Unlike previous versions, v3 of the preprocessor no longer relies on rust-analyzer!
> This means you can safely remove steps related to rust-analyzer from your pipelines.

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
          # the action does not officially support the preprocessor as a tool,
          # but it can use cargo-binstall as a fallback method

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
<!-- prettier-ignore-end -->
