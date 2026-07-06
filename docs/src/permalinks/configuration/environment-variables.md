# Environment variables

You can set the following environment variables to change the preprocessor's behavior.

## `CI`

{% include "/docs/src/_snippets/environment-variables/ci.md" %}

## `MDBOOK_LOG`

<!-- prettier-ignore-start -->
{% with preprocessor = "mdbook-permalinks" %}
  {% include "/docs/src/_snippets/environment-variables/mdbook-log-examples.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

{% include "/docs/src/_snippets/environment-variables/mdbook-log.md" %}

## `MDBOOKKIT_TERM_GRAPHICAL`

<!-- prettier-ignore-start -->
{% filter replace("crates/mdbook-permalinks/tests/book_mdbookkit_term_ascii/", "") %}
{% filter replace("crates/mdbook-permalinks/tests/book_mdbookkit_term_unicode/", "") %}
{% filter replace("crates/mdbook-permalinks/tests/book_mdbookkit_term_logging/", "") %}
{% with preprocessor = "mdbook-permalinks" %}
  {% include "/docs/src/_snippets/environment-variables/mdbook-term-graphical.md" %}
{% endwith %}
{% endfilter %}{% endfilter %}{% endfilter %}
<!-- prettier-ignore-end -->

## `NO_COLOR`, `FORCE_COLOR`

{% include "/docs/src/_snippets/environment-variables/color.md" %}
