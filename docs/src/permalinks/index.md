# mdbook-permalinks

<div class="hidden">

**For best results, view this page at <https://docs.tonywu.dev/mdbookkit/permalinks>.**

</div>

Permalinks in [mdBook]!

With this [preprocessor], you can easily link to any file in your Git repository in your
mdBook documentation, without having to hard-code URLs or worry about broken links.

You simply write ...

```md
Here is a link to the project's [Cargo.toml](/Cargo.toml).
```

... and you will get:

<figure class="fig-text">

Here is a link to the project's [Cargo.toml](/Cargo.toml).

</figure>

## Overview

Follow the [quickstart tutorial](tutorial.md) to try out the preprocessor!

- [Use file paths as links](tutorial.md#linking-by-paths) and get links to your Git
  repository.

- Links are anchored to the Git commit at the time your book is built.

- Display [images](tutorial.md#images) and [media files](tutorial.md#html-links) in your
  repository.

- Supports repositories on GitHub, Codeberg, and Tangled out of the box, but you can
  also
  [define your own permalink format](how-to/remote-url.md#using-a-custom-permalink-format).

- [Get warnings](tutorial.md#check) when links become broken.

<figure>

{% filter replace("crates/mdbook-permalinks/tests/book_tutorial_check/", "") %}
{% include "/crates/mdbook-permalinks/tests/book_tutorial_check/stderr/data.svg" %}
{% endfilter %}

<figcaption>

Link rot happens all the time. The preprocessor will tell you about it.

</figcaption>

</figure>

## License

This project is released under the [Apache 2.0 License](/LICENSE-APACHE.md) and the
[MIT License](/LICENSE-MIT.md).

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
<!-- prettier-ignore-end -->
