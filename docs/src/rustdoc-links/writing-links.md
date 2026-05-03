# Writing links

<div class="hidden">

**For best results, view this page at
<https://docs.tonywu.dev/mdbookkit/rustdoc-links/writing-links>.**

</div>

Because the preprocessor utilizes rustdoc under the hood, it supports the full range of
[intra-doc link][intra-doc-link] syntax that rustdoc currently supports.

This page offers a tour of the different link formats, as well as some writing tips. For
the most up-to-date information regarding rustdoc, please consult the [official
documentation][rustdoc].

> [!TIP]
>
> This page is about correctly formatting Markdown links so that they can be parsed and
> processed by the preprocessor. For guidance on correctly _naming items_ so that they
> can be resolved successfully, please see [Naming items](naming-items.md).

## Overview

Here is a quick look of the most common link formats:

```md
- [`std::path`] is a shortcut link
- [`fmt::Result`][std::fmt::Result] is a reference link
- [`io::Result`][io-result] is a reference link with reusable definition
- [**`std::borrow`**] is a shortcut link with extra inline markup
- [Performance][std::collections#performance] links to a subheading
- [`Box<T>`] is a link with a generic parameter
- [`std::vec!`] and [`mod@std::vec`] have disambiguators

[io-result]: std::io::Result
```

<figure class="fig-text">

- [`std::path`] is a shortcut link
- [`fmt::Result`][std::fmt::Result] is a reference link
- [`io::Result`][io-result] is a reference links with reusable definition
- [**`std::borrow`**] is a shortcut link with extra inline markup
- [Performance][std::collections#performance] links to a subheading
- [`Box<T>`] is a link with a generic parameter
- [`std::vec!`] and [`mod@std::vec`] have disambiguators

[io-result]: std::io::Result

</figure>

## Markdown link syntax

### Shortcut links

In its simplest form, an intra-doc link is just a Rust item wrapped in square brackets.
This is called a ["shortcut" link][shortcut-link]. In this case, the item name is also
the displayed link text:

> ```md
> - [u8], the 8-bit unsigned integer type.
> - [std::option], optional values
> ```
>
> - [u8], the 8-bit unsigned integer type.
> - [std::option], optional values

Backticks are supported, and the link will display as inline code:

> ```md
> [`std::collections::BTreeMap`], an ordered map based on a B-Tree.
> ```
>
> [`std::collections::BTreeMap`], an ordered map based on a B-Tree.

If you don't intend a bracketed text to be a link, you can escape it using backslashes:

> ```md
> - ([never] gonna give you up)
> - (\[never\] gonna give you up)
> ```
>
> - ([never] gonna give you up)
> - (\[never\] gonna give you up)

> [!TIP]
>
> As a convenience feature, with this preprocessor, other types of inline markup are
> also supported:
>
> > ```md
> > - [_`std::alloc`_], memory _allocation_ APIs
> > - [**`std::borrow`**], a module for working with **borrowed** data
> > - [~~`std::mem::uninitialized`~~] is deprecated since 1.39.0
> > ```
> >
> > - [_`std::alloc`_], memory _allocation_ APIs
> > - [**`std::borrow`**], a module for working with **borrowed** data
> > - [~~`std::mem::uninitialized`~~] is deprecated since 1.39.0
>
> Note that this is _not_ supported in rustdoc itself. To maintain compatibility with
> rustdoc, you can place markup outside of the link to achieve the same effect:
>
> > ```md
> > - **[`std::borrow`]**, a module for working with **borrowed** data
> > ```
> >
> > - **[`std::borrow`]**, a module for working with **borrowed** data

### Reference links

To display text that is different from the item name, you can use a ["reference"
link][reference-link]:

> ```md
> The [`option`][std::option] and [`result`][std::result] modules define optional and
> error-handling types.
> ```
>
> The [`option`][std::option] and [`result`][std::result] modules define optional and
> error-handling types.

Unlike in proper Markdown, it is not necessary for the link to also have a corresponding
[link definition][link-definition]. In this case, the [link label][link-label] (text in
the second pair of brackets) is used as the item name.

Still, link definitions may be useful for reusing links in many places. Note that in the
following example, the link labels are also the displayed text:

> ```md
> The most core part of the [`std::io`] module is the [`Read`] and [`Write`] traits ...
> Because they are traits, [`Read`] and [`Write`] are implemented by a number of other
> types, and you can implement them for your types too.
>
> [`Read`]: std::io::Read
> [`Write`]: std::io::Write
> ```
>
> The most core part of the [`std::io`] module is the [`Read`] and [`Write`] traits ...
> Because they are traits, [`Read`] and [`Write`] are implemented by a number of other
> types, and you can implement them for your types too.
>
> [`Read`]: std::io::Read
> [`Write`]: std::io::Write

### Inline links

Finally, if you prefer, you can also use the commonly-seen ["inline" link][inline-link]
syntax. For this preprocessor (and rustdoc), they are functionally the same as
[reference links](#reference-links):

> ```md
> The [`iter`](std::iter) module defines Rust’s iterator trait.
> ```
>
> The [`iter`](std::iter) module defines Rust’s iterator trait.

Use inline links if you would like to specify [link titles][link-title], which are text
tooltips that will appear when you hover on a link using the mouse pointer. If you don't
specify one (that is, in most cases), then rustdoc will by default provide one based on
the resolved link. For example, on desktop, hover on the following links to see the
different titles:

<!-- prettier-ignore-start -->

> ```md
> - Common types of I/O, including [files][std::fs::File] ...
> - Common types of I/O, including [files](std::fs::File "An object providing access to an open file on the filesystem.") ...
> ```
>
> - Common types of I/O, including [files][std::fs::File] ...
> - Common types of I/O, including [files](std::fs::File "An object providing access to an open file on the filesystem.") ...

<!-- prettier-ignore-end -->

## URL fragments (subheadings)

Just like with rustdoc, you can append URL fragments to intra-doc links to link to a
specific subheading on the destination page. You can use fragments in all 3 types of
links mentioned above.

> ```md
> Remember to review the [performance characteristics][std::collections#performance] of
> the different collection types!
> ```
>
> Remember to review the [performance characteristics][std::collections#performance] of
> the different collection types!

## Generic parameters

Item names can contain generic parameters:

> ```md
> [`Vec<T>`], a heap-allocated _vector_ that is resizable at runtime.
> ```
>
> [`Vec<T>`], a heap-allocated _vector_ that is resizable at runtime.
>
> ```md
> | Phantom type               | variance of `T`   |
> | :------------------------- | :---------------- |
> | [`PhantomData<&'a mut T>`] | **in**variant     |
> | [`PhantomData<fn(T)>`]     | **contra**variant |
> ```
>
> | Phantom type               | variance of `T`   |
> | :------------------------- | :---------------- |
> | [`PhantomData<&'a mut T>`] | **in**variant     |
> | [`PhantomData<fn(T)>`]     | **contra**variant |

Do note some caveats with this syntax though:

- [Escaping generic parameters](#escaping-generic-parameters)
- [Some generics syntax is unsupported](#unsupported-generic-parameters-syntax)

## Namespaces and disambiguators

Rust allows different kinds of items to [share the same name][namespace] in the same
scope. In rustdoc (and this preprocessor), you can clarify the kind of item you want to
link to using _disambiguators,_ which takes the form `disambiguator@item`. For example,

> ```md
> `tracing_subscriber::fmt` is both a [function][fn@tracing_subscriber::fmt] and a
> [module][mod@tracing_subscriber::fmt].
> ```
>
> `tracing_subscriber::fmt` is both a [function][fn@tracing_subscriber::fmt] and a
> [module][mod@tracing_subscriber::fmt].

You can find the full [list of supported disambiguators][disambiguators] in the official
documentation.

In the case of [shortcut links](#shortcut-links), the disambiguator will be stripped
from the displayed text:

> ```md
> You can derive the [`trait@PartialEq`] trait using the [`derive@PartialEq`] macro.
> ```
>
> You can derive the [`trait@PartialEq`] trait using the [`derive@PartialEq`] macro.

Additionally, you can indicate that an item is a function by adding `()` after the
function name, or a macro by adding `!` after the macro name:

> ```md
> There is the [`vec!`] macro, and then there is the [`vec`][mod@std::vec] module.
> ```
>
> There is the [`vec!`] macro, and then there is the [`vec`][mod@std::vec] module.

If you don't specify a namespace, and the item is indeed ambiguous, rustdoc will report
a warning:

<figure>
{{#include media/ambiguous-link.svg}}
</figure>

## Caveats & tips

### Broken inline links

> [!WARNING]

Compared to [reference links](#reference-links), [inline links](#inline-links) may pose
a slight hazard when an item fails to resolve: a broken reference link will appear
broken when rendered, while a broken inline link will appear as a clickable link, albeit
with an invalid destination. Although in both cases, the preprocessor should still
produce a diagnostic warning about the broken link.

Compare the following links with typos:

```md
- The [`threads`][std::threads] module contains Rust’s threading abstractions.
- The [`threads`](std::threads) module contains Rust’s threading abstractions.
```

- The \[`threads`\]\[std::threads\] module contains Rust’s threading abstractions.
- The <a href="std::threads"><code>threads</code></a> module contains Rust’s threading
  abstractions.

### Escaping generic parameters

When combining the [shortcut link](#shortcut-links) syntax with the generic parameters
syntax, you should quote the link text in inline code. Otherwise, generic parameters may
be interpreted as (invalid) HTML tags and become invisible.

<figure>
{{#include ../../../crates/mdbook-rustdoc-links/tests/book_link_syntax_escape_generics/stderr/data.svg}}
</figure>

### Unsupported generic parameters syntax

Fully-qualified syntax and the `Fn(T)` special syntax are not supported.

In particular, this means it is currently not possible to link to a specific
implementation of a generic trait, such as the
[`impl From<Ipv6Addr> for IpAddr`](https://doc.rust-lang.org/stable/std/net/enum.IpAddr.html#method.from-1)
in the example below.

<figure>
{{#include ../../../crates/mdbook-rustdoc-links/tests/book_link_syntax_unsupported_generics/stderr/data.svg}}
</figure>

<!-- prettier-ignore-start -->
[intra-doc-link]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[rustdoc]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html
[shortcut-link]: https://spec.commonmark.org/0.31.2/#shortcut-reference-link
[reference-link]: https://spec.commonmark.org/0.31.2/#reference-link
[link-definition]: https://spec.commonmark.org/0.31.2/#link-reference-definition
[link-label]: https://spec.commonmark.org/0.31.2/#link-label
[inline-link]: https://spec.commonmark.org/0.31.2/#inline-link
[link-title]: https://spec.commonmark.org/0.31.2/#link-title
[namespace]: https://doc.rust-lang.org/reference/names/namespaces.html
[disambiguators]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html#namespaces-and-disambiguators
<!-- prettier-ignore-end -->
