# How to set the URL to your repository

For the preprocessor to be able to generate permalinks, it must first know where your
Git repository can be accessed online. This guide will go over the different ways you
can configure this.

## "Well-known" Git forges

The preprocessor tries to make this configuration as easy as possible. However, these
convenient features assume that your repo is hosted on one of the several "well-known"
sites. If you are not using any of the following sites, feel free to skip to
[Using a custom permalink format](#using-a-custom-permalink-format) instead.

The forges that the preprocessor has built-in support for are
[GitHub](https://github.com), [Codeberg](https://codeberg.org), and
[tangled](https://tangled.org).

## Setting `git-repository-url`

You can configure the URL by setting the `output.html.git-repository-url` option. For
example:

```toml config-example
[output.html]
git-repository-url = "https://github.com/me/awesome-book"

[preprocessor.permalinks]
```

> [!IMPORTANT]
>
> This option is set **under the `[output.html]` table** instead of the preprocessor
> table!

The `git-repository-url` option is part of mdBook's builtin [HTML renderer
options][mdbook-html]. When set, mdBook renders a button on the right side of the top
menu bar, which will open the repo at the configured URL. Chances are, you are already
using it, in which case no further setup is required!

It is OK to use a subpage for the URL. The following example uses a URL that opens the
`docs` directory in the `tangled.org/core` repository:

```toml config-example
[output.html]
git-repository-url = "https://tangled.org/tangled.org/core/blob/master/docs"
```

So long as the URL provides the repo's owner and name, the preprocessor will be able to
use it.

## Configuring `git remote`

If you wish not to use the `git-repository-url` option, the preprocessor can
automatically derive the permalink format by looking at the remote URL as configured
through `git`. In this case, no further configuration is required in `book.toml` (other
than, of course, enabling the preprocessor).

You can confirm that your repository has a remote URL set by running the following git
command:

```bash
git remote get-url --no-push origin
```

> [!NOTE]
>
> If your remote repository has separate URLs for pushing versus fetching, the
> preprocessor will prefer the fetch URL.

Both HTTPS URLs and scp-style URLs ("SSH remotes" like `git@github.com:org/repo.git`)
are supported.

By default, the preprocessor will look at the remote named `origin`. You can override
this and use a differently-named remote by setting the `remote-name` option.

## Using a custom permalink format

Finally, in case you are not using any of the forges with built-in support, the
preprocessor allows you to fully customize the format of the generated permalinks via
the `repo-url-template` option. For example:

```toml config-example
[preprocessor.permalinks]
repo-url-template = "https://gitlab.haskell.org/ghc/ghc/-/{tree}/{ref}/{path}"
```

Set the option to the URL to your repository. In this URL, you can include the following
placeholders, which the preprocessor will substitute with actual values during build:

| Placeholder | Replaced with                                                                          |
| :---------: | :------------------------------------------------------------------------------------- |
|  `{tree}`   | The string `tree` or `raw`, depending on where the link is being used.                 |
|   `{ref}`   | The commit SHA (or tag name) your repo was checked out at <br> when the book is built. |
|  `{path}`   | Path to the linked file, starting from repo root.                                      |

With the above example, the preprocessor will generate links such as: [^sha-shortened]

- [<code>https://gitlab.haskell.org/ghc/ghc/-/<strong>tree</strong>/<strong>7ab9028</strong>/<strong>.editorconfig</strong></code>](https://gitlab.haskell.org/ghc/ghc/-/tree/7ab9028/.editorconfig)
- [<code>https://gitlab.haskell.org/ghc/ghc/-/<strong>raw</strong>/<strong>7ab9028</strong>/<strong>docs/users_guide/images/prof_scc.svg</strong></code>](https://gitlab.haskell.org/ghc/ghc/-/raw/7ab9028/docs/users_guide/images/prof_scc.svg)
  (when used in an image)

As a comparison, the following configuration defines GitHub's permalink format (which is
supported by default, without requiring you to use this option):

```toml config-example
[preprocessor.permalinks]
repo-url-template = "https://github.com/tonywu6/mdbookkit/{tree}/{ref}/{path}"
```

> [!IMPORTANT]
>
> If you are using this option, you will need to hard-code essential information such as
> the name of the owner/organization and the repository itself (which should mostly be
> unchanging anyway).
>
> There is **no ~~`{owner}`~~ or ~~`{repo}`~~ placeholder,** and the preprocessor will
> not attempt to guess such information.

The `repo-url-template` option supports further tweaking. Please see
[the reference](../configuration.md#repo-url-template) for further information.

<!-- prettier-ignore-start -->
[mdbook-html]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
<!-- prettier-ignore-end -->

[^sha-shortened]:
    The commit SHAs are truncated in these examples. In actual usage, the preprocessor
    will emit the full SHA.
