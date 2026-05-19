# Environment variables

You can set the following environment variables to change the preprocessor's behavior.

## `CI`

The preprocessor's default behaviors changes when it detects it is running in a
continuous integration (CI) environment via the `CI` environment variable.

See the [continuous integration guide](../how-to/continuous-integration.md) for detailed
information on using the preprocessor in CI.

You usually don't need to set this manually, since most platforms will set `CI=true` by
default.

## `MDBOOK_LOG`

<!-- prettier-ignore-start -->
{% with preprocessor = "mdbook-rustdoc-links" %}
  {% include "/docs/src/_snippets/mdbook-log-examples.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

Control the preprocessor's logging output.

## `MDBOOKKIT_TERM_GRAPHICAL`

## `NO_COLOR`, `FORCE_COLOR`
