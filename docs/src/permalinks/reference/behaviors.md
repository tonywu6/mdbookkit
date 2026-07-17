# Linking behaviors

This page documents how the preprocessor behaves when it comes to various special cases.

## "Book links" vs. "Repo links"

Broadly speaking, the preprocessor separates hyperlinks that it supports into two
categories:

- **"Book links"** are links that point to a location within your mdBook project. For
  example,
  [<code class="nowrap">../_media/example-video.webm</code>](../_media/example-video.webm)
  is a "book link."

  Files pointed to such links are expected to be present in the mdBook build output,
  either as HTML files built from Markdown files, or as other types of assets which
  mdBook copies verbatim. As such, the preprocessor will only validate such links, but
  not alter them.

- **"Repo links"** are links that point to a location within your Git repository but
  outside your book. For example,
  [<code class="nowrap">/docs/book.toml</code>](/docs/book.toml) is a "repo link."

Besides links that are specified as file paths, the preprocessor also processes and
verifies [eligible links that are hardcoded as URLs](../how-to/hardcoded-links.md). The
preprocessor will first try and extract a file path from them, then treat them as either
book links or repo links.

## Symlinks

The preprocessor is capable of resolving symlinks.

When a hyperlink points to a symlink, or a location behind a (directory) symlink:

- If the hyperlink is a _book link_ (the symlink itself is within the book), then the
  preprocessor will keep the link unchanged (instead of rewriting it to the path's
  canonical location). This is because mdBook already recursively copies everything
  behind a symlink to the output directory.

- If the hyperlink is a _repo link_ (the symlink itself is outside the book), then the
  preprocessor will first resolve the symlink to its canonical path, and then process
  the hyperlink as if it were written with that path. Resolving symlinks is preferred
  because most Git forges treat symlinks as regular files when serving content, meaning
  they do not provide redirections.

## Links to book pages

"Book pages" are Markdown files within the `src` directory of your mdBook project (as
opposed to other static files such as images). For example,
[<code class="nowrap">./configuration.md</code>](./configuration.md) is a link to a book
page.

Links to book pages receive special treatment due to how mdBook interacts with Markdown
files.

- For such a link to be considered valid, the destination Markdown file must have been
  mentioned in the
  [`SUMMARY.md` file](https://rust-lang.github.io/mdBook/format/summary.html).

  As of mdBook 0.5.4, Markdown files that are not present in `SUMMARY.md` are neither
  compiled into HTML files nor copied to output. In other words, it is not possible to
  link to such files as static assets (as if they were images or videos).

- For a link to a directory to be considered valid, the directory must contain an
  `index.md` file
  [(or a `README.md` file)](https://github.com/rust-lang/mdBook/blob/v0.5.4/crates/mdbook-driver/src/builtin_preprocessors/index.rs).

- Paths to book pages can either contain the `.md` file extension, the `.html`
  extension, or no extension at all. The preprocessor will
  [probe several candidate paths](/crates/mdbook-permalinks/src/main.rs#L753-L781) for
  each link to determine whether the link is valid.

## HTML attributes

The preprocessor supports converting and validating links in common HTML attributes.

More specifically, the preprocessor converts links found in HTML to either
[webpage links or raw links](../getting-started.md#html-links) based on the HTML
elements and attributes used. Notable rules are:

| CSS selector        | Link type |
| :------------------ | :-------- |
| `a[href][download]` | Raw       |
| `[href]`            | Webpage   |
| `data[object]`      | Raw       |
| `[src]`             | Raw       |

The exact rules is
[specified in the source code](/crates/mdbook-permalinks/src/link.rs#L412-L433).
