- [`text::get_width`]
  - Known issue: `#[doc(inline)]` has no effect here and this item will not have a link.
    However, rustdoc will not emit a warning, because the item is resolvable as far as
    rustc is concerned, the link is not generated because rustdoc cannot find the
    destination HTML file locally. Note that this is reproducible only if the target
    crate does not set [`#![doc(html_root_url)]`][html_root_url].
- [`calc::add`](https://docs.rs/calc/0.1.0/calc/fn.add.html "fn calc::add")

- [`utf8_width::get_width`]
- [`pin_project_lite::pin_project`]

<!-- prettier-ignore-start -->
[html_root_url]: https://doc.rust-lang.org/stable/rustdoc/write-documentation/the-doc-attribute.html#html_root_url
<!-- prettier-ignore-end -->
