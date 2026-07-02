# How to use the preprocessor in CI

{% include "/docs/src/_snippets/continuous-integration/preface.md" %}

## Things to know

<!-- prettier-ignore-start -->
{% filter replace("crates/mdbook-permalinks/tests/fail_on_warnings_in_ci/", "") %}
{% filter replace("crates/mdbook-permalinks/tests/book_mdbookkit_term_logging/", "") %}
  {% with
    exit_status = "/crates/mdbook-permalinks/tests/fail_on_warnings_in_ci/stderr/data.svg",
    log_messages = "/crates/mdbook-permalinks/tests/book_mdbookkit_term_logging/stderr/data.svg"
  -%}
    {% include "/docs/src/_snippets/continuous-integration/things-to-know.md" %}
  {%- endwith %}
{% endfilter %}
{% endfilter %}
<!-- prettier-ignore-end -->

## Tips

This section lists some usual prerequisites for running the preprocessor in CI, as well
as some example pipeline configurations that you can use as starting points.

### Installing the preprocessor

You can install a precompiled version of the preprocessor using [`cargo binstall`], or
fetch it from [GitHub Releases][gh-releases]. As usual, you can also compile from source
by running `cargo install`.

For GitHub Actions, actions like [`taiki-e/install-action`] may be useful for installing
both mdBook and the preprocessor in one go.

### Ensuring Git information

Since the preprocessor relies on the presence of a local Git repository to perform link
resolution and checking, you should make sure that your repository is properly
replicated in your CI environment. This should be true most of the time, as long as you
are using your provider's default method of checking out.

#### Ensuring Git tags

One specific detail to note is about **Git tags.**

When the preprocessor detects that the currently checked-out commit is tagged, then in
the generated permalinks, instead of using the full commit hash, it will use the tag
name, which makes for more readable URLs.

Some CI providers, such as GitHub Actions with [`actions/checkout`], perform shallow
cloning by default, such that tags are not fetched. In such cases, the preprocessor will
always generate permalinks using the full commit hash (the links will still point to the
correct commit, albeit much more verbose).

If you would like to make sure that the generated links feature tag names (e.g. for
release builds), then you should configure your workflow to fetch tags. For example, on
GitHub Actions, this can be done with the `fetch-tags` option[^fetch-tags].

```diff yaml
  - uses: actions/checkout@v6
+   with:
+     fetch-depth: 0
+     fetch-tags: true
```

## Example: GitHub Actions

```yaml
name: Build docs

on:
  release:
    types: [published]

jobs:
  build:
    name: Build
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v6
        with:
          # make sure tags are fetched as well
          # see also https://github.com/actions/checkout/issues/1471
          fetch-depth: 0
          fetch-tags: true

      - name: Install tools
        uses: taiki-e/install-action@v2
        with:
          tool: |
            mdbook
            mdbook-permalinks
          fallback: cargo-binstall
          # the action does not officially support the preprocessor as a tool,
          # but it can use cargo-binstall as a fallback method

      - name: Build docs
        working-directory: docs
        run: |
          mdbook build

      - name: Upload docs
        uses: actions/upload-artifact@v7
        with:
          name: book
          path: docs/book
```

## Example: Tangled Spindles

TODO:

[^fetch-tags]:
    Currently, a [bug](https://github.com/actions/checkout/issues/1471) has prevented
    `fetch-tags` from taking effect unless you also specify `fetch-depth: 0`.

<!-- prettier-ignore-start -->
[`cargo binstall`]: https://github.com/cargo-bins/cargo-binstall
[`taiki-e/install-action`]: https://github.com/taiki-e/install-action/tree/v2/
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
[`actions/checkout`]: https://github.com/actions/checkout
<!-- prettier-ignore-end -->
