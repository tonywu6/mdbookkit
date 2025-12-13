# Motivation

[rustdoc supports linking to items by name][rustdoc]. This is awesome for at least two
reasons:

- It's convenient. It could be as simple as [just adding brackets around
  names][uv-brackets].
- Links generated via rustdoc are version-pinned. Instead of seeing links that default
  to the latest version, where items may have been moved or deleted, you get the
  versions that your packages actually depend on.

mdBook doesn't have the luxury of accessing compiler internals yet, so you are left with
manually sourcing links from [docs.rs](https://docs.rs). Then one of two things could
happen:

- APIs are mentioned without appropriate links to reference docs.

  This may be fine for tutorials and examples. However, readers of your docs will not be
  able to navigate between guides and references as easily as it could have been.

- You do want at least some reference links. It could quickly become cumbersome to find
  and copy the correct links by hand, and even more so to maintain them over time.

`mdbook-rustdoc-links` is the tooling answer to these problems. _Effortless, correct,
and good practice — choose all three!_

> [!TIP]
>
> This style of linking is also known as "intra-doc links" — read more about it in the
> [original RFC][intra-doc-link].

<!-- prettier-ignore-start -->

[intra-doc-link]: https://rust-lang.github.io/rfcs/1946-intra-rustdoc-links.html
[rustdoc]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[uv-brackets]: https://github.com/astral-sh/uv/pull/12076/files

<!-- prettier-ignore-end -->
