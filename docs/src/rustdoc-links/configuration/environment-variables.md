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

## `MDBOOKKIT_LINK_REPORT`

<p><details>
  <summary>Example usage</summary>

```shell
MDBOOKKIT_LINK_REPORT=1 mdbook build
```

</details></p>

If set, the preprocessor will log all Rust items that it has successfully resolved.
Items are deduplicated across the book and sorted alphabetically by item names. Log
items from this feature are prefixed with the string `link-report`. You can use this to,
for example, monitor any changes in the generated links between builds.

<figure>

{% include "/crates/mdbook-rustdoc-links/tests/link_report/stderr/data.svg" %}

<figcaption>
  Example link report
</figcaption>

</figure>

## `NO_COLOR`, `FORCE_COLOR`

{% include "/docs/src/_snippets/environment-variables/color.md" %}
