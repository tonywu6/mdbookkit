<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.{{ preprocessor_name }}]
fail-on-warnings = "always"
```

</details></p>

- type: string, either `"ci"` or `"always"`
- default: `"ci"`

Controls if the preprocessor will exit with a non-zero status code when there are
warnings during build. Warnings include log messages at the `WARN` level and diagnostics
at the `warning` level.

Can be either:

- `"ci"`. The preprocessor will exit with an error if it is
  [running in a CI environment](how-to/continuous-integration.md#detecting-ci) and there
  are warnings.

  If it is not running in CI, the preprocessor prints out warnings but will exit with
  `0` (unless there are errors).

- `"always"`. The preprocessor will always exit with an error as long as there are
  warnings.

The default is `"ci"`. This allows the preprocessor to fail your CI builds if it detects
potential issues, but keep `mdbook serve` running during local development.

Regardless of this option, log messages and diagnostics at the error severity will
always cause the preprocessor to exit with an error.
