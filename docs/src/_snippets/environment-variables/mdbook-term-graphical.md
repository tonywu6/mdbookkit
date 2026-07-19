By default, if either [`CI`](#ci) or [`MDBOOK_LOG`](#mdbook_log) has been set, then the
preprocessor prints diagnostic items as concise logging messages instead of graphically.
You can restore the graphical format using the `MDBOOKKIT_TERM_GRAPHICAL` environment
variable:

<figure>

<figcaption>
  Example diagnostic output when the preprocessor is in logging mode (either <code>CI</code>
  or <code>MDBOOK_LOG</code> has been set).
</figcaption>

```shell
MDBOOK_LOG=warn mdbook build
```

{% include "/crates/" ~ preprocessor ~ "/tests/book_mdbookkit_term_logging/stderr/data.svg" %}

</figure>

<figure>

<figcaption>
  Example diagnostic output using the <code>unicode</code> mode. <br>
  This is the default format when none of <code>CI</code>, <code>MDBOOK_LOG</code>,
  or <code>MDBOOKKIT_TERM_GRAPHICAL</code> is specified.
</figcaption>

```shell
MDBOOKKIT_TERM_GRAPHICAL=unicode MDBOOK_LOG=warn mdbook build
```

{% include "/crates/" ~ preprocessor ~ "/tests/book_mdbookkit_term_unicode/stderr/data.svg" %}

</figure>

<figure>

<figcaption>
  Example diagnostic output using the <code>ascii</code> mode. <br>
  This uses plain ASCII characters instead of line drawing characters.
</figcaption>

```shell
MDBOOKKIT_TERM_GRAPHICAL=ascii MDBOOK_LOG=warn mdbook build
```

{% include "/crates/" ~ preprocessor ~ "/tests/book_mdbookkit_term_ascii/stderr/data.svg" %}

</figure>
