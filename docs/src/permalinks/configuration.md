# Configuration

This page documents all the options that you can use to customize the preprocessor.

Options are specified through your `book.toml` file. Each heading below corresponds to a
configuration key. Unless otherwise specified, the option should be added under the
`[preprocessor.permalinks]` table.

## `output.html.git-repository-url`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[output.html]
git-repository-url = "https://tangled.org/pds.ls/pdsls"

[preprocessor.permalinks]
```

```toml config-example
[output.html]
git-repository-url = "https://codeberg.org/zesterer/chumsky"

[preprocessor.permalinks]
```

<figure>

```toml config-example
[output.html]
git-repository-url = "https://github.com/rust-lang/mdBook/tree/master/guide"

[preprocessor.permalinks]
```

<figcaption>Setting to a sub-page of the repo is supported.</figcaption>

</figure>

</details></p>

- type: string (a URL)
- default: none

The URL to your git repository.

> [!NOTE]
>
> This option is set under the `[build.html]` table.

`git-repository-url` is one of mdBook's built-in [HTML renderer options][mdbook-html].
When set, mdBook renders a button on the right side of the top menu bar.

If your repository is hosted on one of the following supported sites, then the
preprocessor can reuse this setting to know how to generate permalinks:

- [GitHub](https://github.com)
- [Codeberg](https://codeberg.org)
- [Tangled](https://tangled.org)

See the [remote URL guide](how-to/remote-url.md#using-a-custom-permalink-format) for a
walkthrough.

If your forge is supported but you don't set `git-repository-url`, the preprocessor will
try to detect a format by
[checking your `git remote` configuration](how-to/remote-url.md#configuring-git-remote).

If your forge is not supported, you can use the
[`repo-url-template`](#repo-url-template) option to fully customize the permalink
format.

## `output.html.site-url`

<p><details>
  <summary>Example usage</summary>

<figure>

```toml config-example
[output.html]
site-url = "https://docs.example.org"

[preprocessor.permalinks]
```

<figcaption>If your book is hosted at the root path of your website</figcaption>

</figure>

<figure>

```toml config-example
[output.html]
site-url = "https://rust-lang.github.io/mdBook"

[preprocessor.permalinks]
```

<figcaption>If your book is hosted under a sub-path on your website</figcaption>

</figure>

</details></p>

- type: string
- default: none

The base URL where your book is hosted.

> [!NOTE]
>
> This option is set under the `[build.html]` table.

`site-url` is one of mdBook's built-in [HTML renderer options][mdbook-html]. mdBook uses
this option to ensure that links on the 404 page are correct.

Setting this to a fully-qualified URL (that begins with `https://` and contains a domain
name) enables the preprocessor to
[validate links to your book with hardcoded URLs](how-to/hardcoded-links.md#checking-urls-to-your-book).

## `repo-url-template`

<p><details>
  <summary>Example usage</summary>

<figure>

```toml config-example
[preprocessor.permalinks]
repo-url-template = "https://gitlab.haskell.org/ghc/ghc/-/{tree}/{ref}/{path}"
```

<figcaption>

With this template, the preprocessor generates permalinks that would link to the
[GHC repository](https://gitlab.haskell.org/ghc/ghc). GitLab uses a slightly different
permalink format than GitHub's.

</figcaption>

</figure>

<figure>

```toml config-example
[preprocessor.permalinks]
repo-url-template = "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}"
```

<figcaption>

With this template, the preprocessor generates permalinks that would link to the
[Linux kernel](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git). <br>
Note that [cgit], kernel.org's Git frontend, places the reference (commit hash) in the
URL query instead of the path, unlike other common permalink formats.

</figcaption>

</figure>

</details></p>

- type: string (a URL) or [table](#repo-url-templateparams--repo-url-templatetemplate)
- default: detected from environment

Customize the URL format that the preprocessor will use to generate permalinks.

If this option is not set, the preprocessor will try to
[detect a URL format from the environment](#outputhtmlgit-repository-url). See the
[remote URL guide](how-to/remote-url.md#using-a-custom-permalink-format) for a
walkthrough.

If your remote repository is on one of the
[supported sites](#outputhtmlgit-repository-url), you do not need to use the
<code class="nowrap">repo-url-template</code> option. Otherwise, it is required.

### Syntax

Within the template URL, you can specify the following placeholders, which will be
replaced at runtime by their actual values:

{% include "_snippets/repo-url-template-placeholders.md" %}

Placeholders may appear in the URL path, as [query][url-query] values, or in the
[fragment][url-fragment].

> [!IMPORTANT]
>
> Note that there is no ~~`{owner}`~~ or ~~`{repo}`~~ placeholder. You are expected to
> include essential information, such as the repo name, in your template.

> [!NOTE]
>
> Currently, a placeholder must fully occupy a path segment, a query value, or the
> fragment, and cannot have prefixes or suffixes, for example:
>
> - `/{path}`, `?ref={ref}`, and `#{kind}` are valid;
> - `/some-{path}`, `?ref=g{ref}`, and `#{kind};` are not valid and will be ignored.

### `repo-url-template.params` <br> `repo-url-template.template`

Most Git forges require permalinks to contain certain strings to clarify what type of
content is expected from the server. Such disambiguation is supported in the URL
template through the `{tree}` and `{kind}` placeholders.

For example, linking to GitHub requires the `{tree}` placeholder, which is either `tree`
or `raw`:

- A link that has `/tree/` in its path opens GitHub's file preview page, for example:
  <br>
  [<code>https://github.com/tokio-rs/tracing/<strong>tree</strong>/add986d/assets/logo.svg</code>](https://github.com/tokio-rs/tracing/tree/add986d/assets/logo.svg)
- A link that has `/raw/` in its path accesses the file's raw content directly, without
  GitHub's UI, for example:
  [<code>https://github.com/tokio-rs/tracing/<strong>raw</strong>/add986d/assets/logo.svg</code>](https://github.com/tokio-rs/tracing/raw/add986d/assets/logo.svg)

If your remote repository uses texts that are different than the default ones, then you
can use the <code class="nowrap">repo-url-template.params</code> option to customize
them.

For example:

```toml config-example
[preprocessor.permalinks.repo-url-template]
template = "https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/{tree}/{path}?h={ref}"
params.raw = "blob"
params.tree = "tree"
```

This configures the preprocessor to link to kernel.org, which uses [cgit] as the web
frontend. Unlike GitHub, which uses `tree` (or `blob`) to denote a "webpage" permalink
(clickable links), and `raw` to denote a permalink to raw file content (e.g. for an
image), [cgit] serves webpages if the link specifies `tree`, and raw content if the link
[specifies `blob`](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/blob/.rustfmt.toml),
then:

- `params.raw = "blob"` tells the preprocessor to use `blob` for raw permalinks (which
  would otherwise be `raw`);
- `params.tree = "tree"` tells the preprocessor to only use `tree` for webpage
  permalinks.

The default values of `params` are:

```toml config-example
[preprocessor.permalinks.repo-url-template]
params.tree = ["tree", "blob"]
params.raw = "raw"
params.commit = "commit"
params.tag = "tag"
# Codeberg requires `commit` and `tag`
```

You may specify multiple values for each key. The preprocessor will use the first value
in the list when generating links, and consider all values when matching URLs in order
to
[check hardcoded links to your repository](how-to/hardcoded-links.md#checking-urls-to-your-repo)
(so for GitHub, links using either `tree` or `blob` can be checked).

## `always-link`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.permalinks]
always-link = [".rs", ".yml"]
```

</details></p>

- type: array of strings (file extensions)
- default: none

By default, the preprocessor does not convert a link to a permalink if it points to a
path within your book. Because mdBook always copies files within the `src` directory to
the output, such links remain intact.

If you want the preprocessor to always generate permalinks for certain files even if
they are in the book, then you can use the `always-link` option. The option accepts a
list of file extensions, with the leading dot, for example: `.rs`.

## `remote-name`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.permalinks]
remote-name = "upstream"
```

</details></p>

- type: string
- default: `"origin"`

When neither the [`output.html.git-repository-url`](#outputhtmlgit-repository-url) nor
the [`repo-url-template`](#repo-url-template) is specified, to determine a suitable URL
format to use for permalinks, the preprocessor will check the URL of an existing Git
remote. See the [remote URL guide](how-to/remote-url.md#configuring-git-remote) for a
walkthough.

By default, the preprocessor will check the `origin` remote. Specify the `remote-name`
option to use a remote with a different name.

## `dev-mode`

<p><details>
  <summary>Example usage</summary>

```toml config-example
[preprocessor.permalinks]
unstable-features = true
dev-mode = true
```

```toml config-example
[preprocessor.permalinks]
unstable-features = true

[preprocessor.permalinks.dev-mode]
editor-uri = "zed://file/{path}"
```

```toml config-example
[preprocessor.permalinks]
unstable-features = true

[preprocessor.permalinks.dev-mode]
editor-uri = "txmt://open?url={url}"
```

</details></p>

- type: boolean or [table](#dev-modeeditor-uri)
- default: `false`

{% with feature = "`dev-mode`" %}
{% include "/docs/src/_snippets/unstable-features.md" %} {% endwith %}

Enables the development mode.

If enabled, and the preprocessor is running locally (not
[in a CI environment](how-to/continuous-integration.md)), then it will generate links
more suitable for local previewing, instead of the usual permalinks.

See the [local development guide](how-to/local-development.md) for more information.

### `dev-mode.editor-uri`

When `dev-mode` is active, for clickable links, the preprocessor generates URLs that
[open the file or directory in your text editor](how-to/local-development.md#opening-files-in-your-editor).

By default, this editor is VS Code, which [listens to][vscode-uri] the special
`vscode://file/{path}` URI.

If you use a different text editor that also supports this form of URL handling, then
you can use the `dev-mode.editor-uri` option to generate preview links for your editor.
See the
[local development guide](how-to/local-development.md#opening-files-in-your-editor) for
a walkthrough.

For example, to open links in [Zed](https://zed.dev):

```toml config-example
[preprocessor.permalinks]
unstable-features = true

[preprocessor.permalinks.dev-mode]
editor-uri = "zed://file/{path}"
```

Within the template URI, you can specify the following placeholders, which will be
replaced at runtime by their actual values:

| Placeholder | Replaced with                                         |
| :---------: | :---------------------------------------------------- |
|  `{path}`   | The full file path to the linked file or directory.   |
|   `{url}`   | The file path encoded as a [`file://` URI][file-uri]. |

### `dev-mode.embed-images`

When `dev-mode` is active, for links that are used in
[images or videos](tutorial.md#images), the preprocessor generates URLs such that they
can be previewed locally, without needing your browser to access your remote repository.

The preprocessor achieves this using
[data URLs](https://developer.mozilla.org/en-US/docs/Web/URI/Reference/Schemes/data),
which requires fully reading the linked files and encoding the content to Base64.

If this is undesirable, then you may set `dev-mode.embed-images = false` to disable this
behavior.

## `fail-on-warnings`

<!-- prettier-ignore-start -->
{% with preprocessor_name = "permalinks" %}
  {% include "/docs/src/_snippets/fail-on-warnings.md" %}
{% endwith %}
<!-- prettier-ignore-end -->

<!-- prettier-ignore-start -->
[cgit]: https://git.zx2c4.com/cgit/about/
[url-query]: https://developer.mozilla.org/en-US/docs/Web/URI/Reference/Query
[url-fragment]: https://developer.mozilla.org/en-US/docs/Web/URI/Reference/Fragment
[mdbook-html]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
[vscode-uri]: https://code.visualstudio.com/docs/configure/command-line#_opening-vs-code-with-urls
[file-uri]: https://en.wikipedia.org/wiki/File_URI_scheme
<!-- prettier-ignore-end -->
