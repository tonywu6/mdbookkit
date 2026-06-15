# How to use the preprocessor to check hard-coded URLs

You may find that in some situations, you cannot use path-based links, and must fallback
to writing full URLs instead, whether it's for linking to your repo or your book.

For example, a scenario that this project has encountered is with its workspace-level
[README](/README.md) file: I'd like to reuse the README's content for the book's
[homepage](/docs/src):

- While I can simply use the {% raw %} [`{{#include}}`][mdbook-include] {% endraw %}
  directive to do so, I cannot use relative paths as links in the README file, since
  they may become broken after being included.

- Furthermore, while the README is intended to be viewed on GitHub, I'd prefer if the
  documentation-related links in it would open this website instead of the file browser
  on GitHub. For this reason, I am using full URLs in the README file.

Consider also, that you may want to reuse your crate's README file: if you are
publishing your crate, then the README will be displayed on
[crates.io](https://crates.io), where path-based link will not work at all.

In any case, if you elect to hard-code URLs in your book, whether to link to other pages
or to your repository, the preprocessor can validate them during build, and warn you if
they are broken.

## Checking URLs to your repo

To have the preprocessor process URLs that link to your repository, **specify `HEAD`**
where you normally specify the branch name or commit hash.

During build, the preprocessor will substitute `HEAD` with the actual commit hash or tag
name, so that they become permalinks:

> ```md
> Link to [`Cargo.toml`](https://github.com/tonywu6/mdbookkit/tree/HEAD/Cargo.toml)
> ```
>
> Link to [`Cargo.toml`](https://github.com/tonywu6/mdbookkit/tree/HEAD/Cargo.toml)

The preprocessor will validate that the link is correct by extracting and checking the
local file path, using the same rules for path-based links:

> ```md
> The [`target` directory](https://github.com/me/awesome-crate/tree/HEAD/target)
> ```
>
> {% filter replace("crates/mdbook-permalinks/tests/book_hardcoded_repo_link/", "") %}
>
> <figure style="margin: 0;">{% include "/crates/mdbook-permalinks/tests/book_hardcoded_repo_link/stderr/data.svg" %}</figure>{% endfilter %}

> [!IMPORTANT]
>
> Your links must use the reference `HEAD` for the preprocessor to validate them. Links
> using any other ref, **such as the `main` branch, will remain unchanged.**

## Checking URLs to your book

The preprocessor can also check links that point to your published book (i.e. to your
website).

For the preprocessor to recognize links to your book, you must set mdBook's
`output.html.site-url` option to the full URL where your book will be hosted. For
example:

```toml config-example
[output.html]
site-url = "https://me.example.org/my/book"

[preprocessor.permalinks]
```

> [!IMPORTANT]
>
> This option is set **under the `[output.html]` table** instead of the preprocessor
> table!

The `site-url` option is part of mdBook's builtin [HTML renderer options][mdbook-html],
although mdBook only uses it to ensure that links are correct in the 404 page.

By providing a full URL, you enable the preprocessor to extract the corresponding local
file path from any eligible link. The preprocessor can then verify that the path is
accessible:

> ```md
> [Legacy options](https://me.example.org/my/book/api/legacy-options) will be deprecated
> in the near future!
> ```
>
> {% filter replace("crates/mdbook-permalinks/tests/book_hardcoded_book_link_not_found/", "") %}
>
> <figure style="margin: 0;">{% include "/crates/mdbook-permalinks/tests/book_hardcoded_book_link_not_found/stderr/data.svg" %}</figure>{% endfilter %}

With book links, there is some flexibility in terms of how you can specify their paths.
The preprocessor recognizes behaviors that web servers commonly follow when serving
content:

- You can include the `.html` extension in the URL. The preprocessor will remove the
  extension before looking for a matching `.md` file.

- If your URL ends with a [trailing slash][trailing-slash], the preprocessor will check
  whether a matching directory exists and has an `index.md` file. This is based on how
  most web servers and hosting providers will serve the `index.html` file if the link
  reaches a directory.

- For simplicity, you can also completely omit the file extension, in which case the
  preprocessor will check several paths that could potentially provide the given URL,
  including if it is the `index.md` page of a directory: see the above example.

<!-- prettier-ignore-start -->
[mdbook-include]: https://rust-lang.github.io/mdBook/format/mdbook.html#including-files
[mdbook-html]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
[trailing-slash]: https://github.com/slorber/trailing-slash-guide?tab=readme-ov-file#trailing-slash-guide
<!-- prettier-ignore-end -->
