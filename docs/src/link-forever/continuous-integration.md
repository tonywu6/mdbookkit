# Continuous integration

This page gives information and tips for using `mdbook-link-forever` in a continuous
integration (CI) environment.

The preprocessor behaves differently in terms of logging, error handling, etc., when it
detects it is running in CI.

<details class="toc" open>
  <summary>Sections</summary>

- [Detecting CI](#detecting-ci)
- [Linking to Git tags](#linking-to-git-tags)
- [Logging](#logging)
- [Error handling](#error-handling)

</details>

## Detecting CI

To determine whether it is running in CI, the preprocessor honors the `CI` environment
variable. Specifically:

- If `CI` is set to `"true"`, then it is considered in CI[^ci-true];
- Otherwise, it is considered not in CI.

Most major CI/CD services, such as [GitHub Actions][github-actions-ci] and [GitLab
CI/CD][gitlab-ci], automatically configure this variable for you.

## Linking to Git tags

The preprocessor supports both tags and commit SHAs when generating permalinks. The use
of tags is contingent on HEAD being tagged in local Git at build time. You should ensure
that tags are present when building in CI, or the preprocessor will fallback to using
the full SHA (it would still be a permalink, just that it will be more verbose).

For example, in [GitHub Actions][actions/checkout], you can use:

```yaml
steps:
  - uses: actions/checkout@v4
    with:
      fetch-tags: true
      fetch-depth: 0 # https://github.com/actions/checkout/issues/1471#issuecomment-1771231294
```

## Logging

By default, the preprocessor shows a progress spinner when it is running.

When running in CI, progress is instead printed as logs (using [log] and
[env_logger])[^stderr].

You can control logging levels using the [`RUST_LOG`] environment variable.

## Error handling

By default, when the preprocessor encounters any non-fatal issues, such as when a link
fails to resolve, it prints them as warnings but continues to run. This is so that your
book continues to build via `mdbook serve` while you make edits.

When running in CI, all such warnings are promoted to errors. The preprocessor will exit
with a non-zero status code when there are warnings, which will fail your build. This
prevents outdated or incorrect links from being accidentally deployed.

You can explicitly control this behavior using the
[`fail-on-warnings`](configuration.md#fail-on-warnings) option.

[^ci-true]:
    Specifically, when `CI` is anything other than `""`, `"0"`, or `"false"`. The logic
    is encapsulated in the [`is_ci`][crate::env::is_ci] function.

[^stderr]:
    Specifically, when stderr is redirected to something that isn't a terminal, such as
    a file.

<!-- prettier-ignore-start -->

[`RUST_LOG`]: https://docs.rs/env_logger/latest/env_logger/#enabling-logging
[actions/checkout]: https://github.com/actions/checkout
[github-actions-ci]: https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/store-information-in-variables#default-environment-variables
[gitlab-ci]: https://docs.gitlab.com/ci/variables/predefined_variables/
[rustup-ra]: https://rust-analyzer.github.io/book/rust_analyzer_binary.html#rustup

<!-- prettier-ignore-end -->
