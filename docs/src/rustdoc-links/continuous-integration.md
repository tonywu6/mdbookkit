# Continuous integration

The preprocessor optimizes some behaviors for continuous integration (CI) environments,
in terms of error handling, logging, etc.

## Detecting CI

{{#include ../snippets/ci/detecting-ci.md}}

## Installing rust-analyzer

rust-analyzer must be on `PATH` when running in CI[^ra-on-path].

One way to install it is via [rustup][rustup-ra]. For example, in [GitHub
Actions][dtolnay/rust-toolchain], you can use:

```yaml
steps:
  - uses: dtolnay/rust-toolchain@stable
    with:
      components: rust-analyzer
```

> [!NOTE]
>
> rust-analyzer from rustup follows Rust's release schedule, which may lag behind the
> version bundled with the VS Code extension.

## Logging

See [Logging](logging.md).

## Error handling

{{#include ../snippets/ci/error-handling.md}}

[^ra-on-path]:
    You may alternatively specify a command to use for rust-analyzer via the
    [`rust-analyzer`](configuration.md#rust-analyzer) configuration option.

<!-- prettier-ignore-start -->
[dtolnay/rust-toolchain]: https://github.com/dtolnay/rust-toolchain
[rustup-ra]: https://rust-analyzer.github.io/book/rust_analyzer_binary.html#rustup
<!-- prettier-ignore-end -->
