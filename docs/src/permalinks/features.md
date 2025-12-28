# Features

## Permalinks

Simply use **relative paths** to link to any file in your source tree, and the
preprocessor will convert them to permalinks.

> ```md
> This project is dual licensed under the
> [Apache License, Version 2.0](../../../LICENSE-APACHE.md) and the
> [MIT (Expat) License](../../../LICENSE-MIT.md).
> ```
>
> This project is dual licensed under the
> [Apache License, Version 2.0](../../../LICENSE-APACHE.md) and the
> [MIT (Expat) License](../../../LICENSE-MIT.md).

> [!TIP]
>
> Not only is linking by paths well-supported by platforms such as
> [GitHub][github-relative-links], but editors like VS Code also provide smart features
> like [path completions][vscode-path-completions] and [link
> validation][link-validation].

**URL fragments** are preserved. For example, you may use fragments to link to specific
lines, if your Git hosting provider supports it:

> ```md
> This book uses [esbuild] to
> [preprocess its style sheet](../../app/build/build.ts#L13-25).
> ```
>
> This book uses [esbuild] to
> [preprocess its style sheet](../../app/build/build.ts#L13-25).

By default, links to files under your book's `src/` directory are not converted, since
mdBook already [copies them to build output][mdbook-src-build]. This is configurable
using the [`always-link`](configuration.md#always-link) option.

## Versioning

Permalinks are **versioned using the tag name or hash** of the commit from which the
book was built. Your links remain consistent with their source commit even as content in
your repository changes over time.

## Repo URL auto-discovery

To determine the base URL of the generated permalinks, the preprocessor looks at the
following places and uses the first one it finds:

1. The [`output.html.git-repository-url`] option in your `book.toml`
2. The URL of a Git remote named `origin`[^1]

> [!TIP]
>
> For Git remotes, both HTTP URLs and "scp-like" URIs (`git@github.com:org/repo.git`)
> are supported.

Currently, the preprocessor automatically uses repo URLs from the following providers:

- GitHub, `https://github.com/*`

Alternatively, you may configure a custom URL format using the
[`repo-url-template`](configuration.md#repo-url-template) option.

## Markdown images

Providers such as GitHub support two types of permalinks:

- a `tree` or `blob` URL that opens the file's webpage for viewing
- a `raw` URL that directly serves the file's content, suitable for embedding or
  downloading

The preprocessor detects the context in which a link appears and selects the most
appropriate type of URL to use: `tree` if it is a clickable link, or `raw` if it is for
an image.

For example, the following snippet creates an image wrapped in a clickable link which
opens the image's page on GitHub:

> ```md
> [![Minato City][minato-city]][minato-city]
>
> [minato-city]: /crates/mdbook-permalinks/src/tests/Minato_City,_Tokyo,_Japan.jpg
> ```
>
> [![Minato City][minato-city]][minato-city]
>
> [minato-city]: /crates/mdbook-permalinks/src/tests/Minato_City,_Tokyo,_Japan.jpg

## Link validation

The preprocessor validates any path-based links and notifies you if they are broken.

<figure>

![warnings emitted for broken links](media/error-reporting.png)

<figcaption>

Formatting of diagnostics powered by [miette]

</figcaption>

</figure>

<!-- prettier-ignore-start -->
[`output.html.git-repository-url`]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
[esbuild]: https://esbuild.github.io
[github-relative-links]: https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#relative-links
[link-validation]: https://code.visualstudio.com/docs/languages/markdown#_link-validation
[mdbook-src-build]: https://rust-lang.github.io/mdBook/guide/creating.html#source-files
[vscode-path-completions]: https://code.visualstudio.com/docs/languages/markdown#_path-completions
<!-- prettier-ignore-end -->

[^1]: The remote must be exactly named `origin`. No other name is recognized.
