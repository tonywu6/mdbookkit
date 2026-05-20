# Environment variables

You can set the following environment variables to change the preprocessor's behavior.

## `CI`

<p><details>
  <summary>Example usage</summary>

<figure>

<figcaption>
  Most platforms will configure the <code>CI</code> variable by default. These examples
  are for illustrative purposes, or if you would like to test the preprocessor's behavior locally.
</figcaption>

```shell
CI=1 mdbook build
```

```shell
CI=true mdbook build
```

```shell
CI=false mdbook build
```

<figcaption>
  The preprocessor runs in CI mode as long as the variable has any value, even <code>false</code>.
</figcaption>

```shell
CI= mdbook build
```

<figcaption>
  Setting <code>CI</code> to an empty value disables CI mode.
</figcaption>

</figure>

</details></p>

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

The same variable also controls logging output of the main `mdbook` executable.

The preprocessor uses the `tracing` family of crates for logging. See [its
documentation][EnvFilter#directives] for more details on how to customize this
environment variable.

## `MDBOOKKIT_TERM_GRAPHICAL`

By default, if either [`CI`](#ci) or [`MDBOOK_LOG`](#mdbook_log) has been set, then the
preprocessor prints diagnostic items as concise logging messages instead of graphically.
You can restore the graphical format using the `MDBOOKKIT_TERM_GRAPHICAL` environment
variable:

<figure>

```shell
MDBOOK_LOG=warn mdbook build
```

{% include "/crates/mdbook-rustdoc-links/tests/book_mdbookkit_term_logging/stderr/data.svg" %}

<figcaption>
  Example diagnostic output when the preprocessor is in logging mode (either <code>CI</code>
  or <code>MDBOOK_LOG</code> has been set).
</figcaption>

</figure>

<figure>

```shell
MDBOOKKIT_TERM_GRAPHICAL=unicode MDBOOK_LOG=warn mdbook build
```

{% include "/crates/mdbook-rustdoc-links/tests/book_mdbookkit_term_unicode/stderr/data.svg" %}

<figcaption>
  Example diagnostic output using the <code>unicode</code> mode. <br>
  This is the default format when none of <code>CI</code>, <code>MDBOOK_LOG</code>,
  or <code>MDBOOKKIT_TERM_GRAPHICAL</code> is specified.
</figcaption>

</figure>

<figure>

```shell
MDBOOKKIT_TERM_GRAPHICAL=ascii MDBOOK_LOG=warn mdbook build
```

{% include "/crates/mdbook-rustdoc-links/tests/book_mdbookkit_term_ascii/stderr/data.svg" %}

<figcaption>
  Example diagnostic output using the <code>ascii</code> mode. <br>
  This uses plain ASCII characters instead of line drawing characters.
</figcaption>

</figure>

## `NO_COLOR`, `FORCE_COLOR`

By default, the preprocessor enables or disables colored output depending on whether the
output destination supports it. For example, if stderr is redirected to a file, then the
preprocessor suppresses colors. You can set the `NO_COLOR` or `FORCE_COLOR` environment
variable to explicitly control this.
