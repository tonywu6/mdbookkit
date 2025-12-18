When running locally, when the preprocessor encounters any non-fatal issues, such as
when a link fails to resolve, it prints them as warnings but continues to run. This is
so that your book continues to build via `mdbook serve` while you make edits.

**When running in CI, all such warnings are promoted to errors by default.** The
preprocessor will exit with a non-zero status code which will fail your build. This is
to prevent outdated or incorrect links from being accidentally deployed.

You can explicitly control this behavior using the
[`fail-on-warnings`](configuration.md#fail-on-warnings) option.
