# Environment variables

You can set the following environment variables to change the preprocessor's behavior.

## `CI`

{% include "/docs/src/_snippets/environment-variables/ci.md" %}

## `MDBOOK_LOG`

<!-- prettier-ignore-start -->
{% with preprocessor = "mdbook-rustdoc-links" %}
  {% include "/docs/src/_snippets/environment-variables/mdbook-log-examples.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

{% include "/docs/src/_snippets/environment-variables/mdbook-log.md" %}

## `MDBOOKKIT_TERM_GRAPHICAL`

<!-- prettier-ignore-start -->
{% with preprocessor = "mdbook-rustdoc-links" %}
  {% include "/docs/src/_snippets/environment-variables/mdbook-term-graphical.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

## `NO_COLOR`, `FORCE_COLOR`

{% include "/docs/src/_snippets/environment-variables/color.md" %}
