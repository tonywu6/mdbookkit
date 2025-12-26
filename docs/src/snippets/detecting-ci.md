To determine whether it is running in a CI environment, the preprocessor honors the `CI`
environment variable. Specifically:

- If `CI` is set to `"true"`, then it is considered in CI[^ci-true];
- Otherwise, it is considered not in CI.

Providers such as [GitHub Actions][github-actions-ci] and [GitLab CI/CD][gitlab-ci] have
this variable configured by default.

[^ci-true]:
    Specifically, when `CI` is anything other than `""`, `"0"`, or `"false"`. The logic
    is encapsulated in the [`is_ci`][crate::env::is_ci] function.

<!-- prettier-ignore-start -->

[github-actions-ci]: https://docs.github.com/en/actions/reference/workflows-and-actions/variables#default-environment-variables
[gitlab-ci]: https://docs.gitlab.com/ci/variables/predefined_variables/#predefined-variables

<!-- prettier-ignore-end -->
