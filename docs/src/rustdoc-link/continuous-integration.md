# Continuous integration

This page gives information and tips for using `mdbook-rustdoc-link` in a continuous
integration (CI) environment. The preprocessor optimizes some behaviors for CI, in terms
of error handling, logging, etc.

## Detecting CI

To determine whether it is running in a CI environment, the preprocessor honors the `CI`
environment variable. Specifically:

- If `CI` is set to `"true"`, then it is considered in CI[^ci-true];
- Otherwise, it is considered not in CI.

Providers such as [GitHub Actions][github-actions-ci] and [GitLab CI/CD][gitlab-ci] have
this variable configured by default.

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

<!-- TODO: -->

By default, the preprocessor shows a progress spinner when it is running.

When running in CI, progress is instead printed as logs (using [log] and
[env_logger])[^stderr].

You can control logging levels using the [`RUST_LOG`] environment variable.

## Error handling

When running locally, when the preprocessor encounters any non-fatal issues, such as
when a link fails to resolve, it prints them as warnings but continues to run. This is
so that your book continues to build via `mdbook serve` while you make edits.

When running in CI, all such warnings are promoted to errors. The preprocessor will exit
with a non-zero status code which will fail your build. This is to prevent outdated or
incorrect links from being accidentally deployed.

You can explicitly control this behavior using the
[`fail-on-warnings`](configuration.md#fail-on-warnings) option.

[^ra-on-path]:
    You may alternatively specify a command to use for rust-analyzer via the
    [`rust-analyzer`](configuration.md#rust-analyzer) configuration option.

[^ci-true]:
    Specifically, when `CI` is anything other than `""`, `"0"`, or `"false"`. The logic
    is encapsulated in the [`is_ci`][crate::error::is_ci] function.

[^stderr]:
    Specifically, when stderr is redirected to something that isn't a terminal, such as
    a file.

<!-- prettier-ignore-start -->

[`RUST_LOG`]: https://docs.rs/env_logger/latest/env_logger/#enabling-logging
[dtolnay/rust-toolchain]: https://github.com/dtolnay/rust-toolchain
[github-actions-ci]: https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/store-information-in-variables#default-environment-variables
[gitlab-ci]: https://docs.gitlab.com/ci/variables/predefined_variables/
[rustup-ra]: https://rust-analyzer.github.io/book/rust_analyzer_binary.html#rustup

<!-- prettier-ignore-end -->
