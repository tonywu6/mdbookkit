# Continuous integration

The preprocessor optimizes some behaviors for continuous integration (CI) environments,
in terms of error handling, logging, etc.

## Detecting CI

{{#include ../snippets/ci/detecting-ci.md}}

## Linking to Git tags

The preprocessor supports both tags and commit hashes when generating permalinks. **The
use of tags is contingent on HEAD being tagged in local Git at build time.**

You should ensure that tags are present locally when building in CI, otherwise the
preprocessor will fallback to using the full commit hash. The resulting permalinks will
function the same, but they will be more verbose.

For example, in [GitHub Actions][actions/checkout], you can use:

```yaml
steps:
  - uses: actions/checkout@v4
    with:
      fetch-tags: true
      fetch-depth: 0 # https://github.com/actions/checkout/issues/1471#issuecomment-1771231294
```

## Error handling

{{#include ../snippets/ci/error-handling.md}}

<!-- prettier-ignore-start -->

[`RUST_LOG`]: https://docs.rs/env_logger/latest/env_logger/#enabling-logging
[actions/checkout]: https://github.com/actions/checkout
[github-actions-ci]: https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/store-information-in-variables#default-environment-variables
[gitlab-ci]: https://docs.gitlab.com/ci/variables/predefined_variables/
[rustup-ra]: https://rust-analyzer.github.io/book/rust_analyzer_binary.html#rustup

<!-- prettier-ignore-end -->
