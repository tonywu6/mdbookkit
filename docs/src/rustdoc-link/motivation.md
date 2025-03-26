# Motivation

[rustdoc supports linking to items by name][rustdoc], a.k.a. [intra-doc
links][intra-doc-link]. This is awesome for at least two reasons:

- It's convenient. It could be as simple as [just adding brackets][uv-brackets].
- [Docs.rs](https://docs.rs) will generate cross-crate links that are version-pinned.

mdBook doesn't have the luxury of accessing compiler internals yet, so you are left with
manually sourcing links from docs.rs. Then one of two things could happen:

- APIs are mentioned without linking to reference docs.

  This is probably fine for tutorials and examples, but it does mean a reduced
  connection between guide-level text and usage details. Readers won't be able to
  navigate from one to the other as easily.

- You do want at least some cross-references, but it is cumbersome to find and copy the
  correct links, and even more so to maintain them.

  Links to docs.rs often use `latest` as the version, which could become out-of-sync
  with your code, especially if they point to third-party or unstable APIs.

`mdbook-rustdoc-link` is the tooling answer to these problems. _Effortless, correct, and
good practice — choose all three!_

> [!NOTE]
>
> That being said, sometimes manually specifying URLs is the best option.
>
> Most importantly, writing links by name means they won't be rendered as such when your
> Markdown source is displayed elsewhere. If your document is also intended for places
> like GitHub or crates.io, then you should probably not use this preprocessor.

<!-- prettier-ignore-start -->

[intra-doc-link]: https://rust-lang.github.io/rfcs/1946-intra-rustdoc-links.html
[rustdoc]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[uv-brackets]: https://github.com/astral-sh/uv/pull/12076/files

<!-- prettier-ignore-end -->
