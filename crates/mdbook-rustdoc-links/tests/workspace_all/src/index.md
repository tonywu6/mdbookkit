- [`text::get_width`]
  - Known issue: `#[doc(inline)]` has no effect here and this item will not have a link.
    However, rustdoc will not emit a warning, because the item is resolvable as far as
    rustc is concerned, the link is not generated because rustdoc cannot find the
    destination HTML file locally. See
    <https://doc.rust-lang.org/stable/rustdoc/write-documentation/the-doc-attribute.html#html_root_url>
- [`calc::add`]

- [`utf8_width::get_width`]
- [`pin_project_lite::pin_project`]
