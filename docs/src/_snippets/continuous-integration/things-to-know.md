Using the preprocessor in CI is largely the same as using it in local development.
However, some of the preprocessor's default behaviors are different when it
[detects](#detecting-ci) it is running in CI.

In most cases, no extra configurations are necessary for the preprocessor to work in CI.

### Exiting with failure

When running locally, if the preprocessor detects non-fatal issues with your book, such
as broken links, it will emit warnings but otherwise exits with a success (0) status.
This is so that mdBook will keep running even as there may be temporary errors as you
are editing your book.

When running in CI, **any warnings emitted during the build process will cause the
preprocessor to exit with a failure (1) status at the end,** in which case mdBook will
also exit early. This way, the preprocessor can fail your pipeline if you accidentally
pushed changes that contained problems.

<figure>
  {% include exit_status %}
  <figcaption>Example console output when the preprocessor is running in CI and has warnings</figcaption>
</figure>

### Diagnostics

When running locally, the preprocessor prints diagnostic messages in a graphical style,
similar to how `rustc` prints them.

When running in CI, the preprocessor prints diagnostic messages as log messages:

<figure>
  {% include log_messages %}
</figure>

You can override this behavior using the
[`MDBOOKKIT_TERM_GRAPHICAL`](../reference/environment-variables.md#mdbookkit_term_graphical)
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
