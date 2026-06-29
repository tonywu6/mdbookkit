<a href="/LICENSE-MIT.md">
  This is a link in an HTML block
</a>

This is an <a href="../book.toml">inline link. **Inline styles** should be
preserved.</a>

> The program should properly rewrite <a
>   title="clippy"
>   href="/clippy.toml"
>   target="_blank">HTML tags that span multiple lines</a>

<a href>This just links to this page</a>

<div href="../..">
  Right now, the program will match any element that has eligible attributes
  regardless of whether the attribute is semantically valid.
</div>

<a href="ignored.rs">This file is copied to output</a>

<p>
  The program should rewrite all these to the path to the Markdown file
  (mdbook is supposed to convert them to end in `.html` during build)

- <a href="./raw-links.md"></a>
- <a href="./raw-links"></a>
- <a href="./raw-links.html"></a>
</p>
