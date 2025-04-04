# Supported syntax

<div class="hidden">

**For best results, view this page at
<https://tonywu6.github.io/mdbookkit/rustdoc-link/supported-syntax>.**

</div>

This page showcases all the syntax supported by `mdbook-rustdoc-link`.

Most of the formats [supported by rustdoc][rustdoc-linking] are supported. Unsupported
syntax and differences in behavior are emphasized below.

In general, specifying items as you would when writing Rust code should "just work".

<details class="toc" open>
  <summary>Sections</summary>

- [Types, modules, and associated items](#types-modules-and-associated-items)
- [Generic parameters](#generic-parameters)
- [Functions and macros](#functions-and-macros)
- [Implementors and fully qualified syntax](#implementors-and-fully-qualified-syntax)
- [Disambiguators](#disambiguators)
- [Special types](#special-types)
- [Struct fields](#struct-fields)
- [Markdown link syntax](#markdown-link-syntax)
- [Linking to page sections](#linking-to-page-sections)

</details>

> [!TIP]
>
> This page is also used for snapshot testing! To see how all the links would look like
> in Markdown after they have been processed, see
> [supported-syntax.snap](/crates/mdbookkit/tests/snaps/rustdoc_link/supported-syntax.snap)
> and
> [supported-syntax.stderr.snap](/crates/mdbookkit/tests/snaps/rustdoc_link/supported-syntax.stderr.snap).

## Types, modules, and associated items

> ```md
> Module [`alloc`][std::alloc] — Memory allocation APIs.
> ```
>
> Module [`alloc`][std::alloc] — Memory allocation APIs.
>
> ```md
> Every [`Option`] is either [`Some`][Option::Some][^1] and contains a value, or
> [`None`][Option::None][^1], and does not.
> ```
>
> Every [`Option`] is either [`Some`][Option::Some][^1] and contains a value, or
> [`None`][Option::None][^1], and does not.
>
> ```md
> [`Ipv4Addr::LOCALHOST`][core::net::Ipv4Addr::LOCALHOST] — An IPv4 address with the
> address pointing to localhost: `127.0.0.1`.
> ```
>
> [`Ipv4Addr::LOCALHOST`][core::net::Ipv4Addr::LOCALHOST] — An IPv4 address with the
> address pointing to localhost: `127.0.0.1`.

## Generic parameters

Types can contain generic parameters. This is _compatible_ with rustdoc.

> ```md
> [`Vec<T>`] — A heap-allocated _vector_ that is resizable at runtime.
> ```
>
> [`Vec<T>`] — A heap-allocated _vector_ that is resizable at runtime.
>
> ```md
> | Phantom type                                       | variance of `T`   |
> | :------------------------------------------------- | :---------------- |
> | [`&'a mut T`][std::marker::PhantomData<&'a mut T>] | **in**variant     |
> | [`fn(T)`][std::marker::PhantomData<fn(T)>]         | **contra**variant |
> ```
>
> | Phantom type                                       | variance of `T`   |
> | :------------------------------------------------- | :---------------- |
> | [`&'a mut T`][std::marker::PhantomData<&'a mut T>] | **in**variant     |
> | [`fn(T)`][std::marker::PhantomData<fn(T)>]         | **contra**variant |

This includes if you use turbofish:

> ```md
> `collect()` is one of the few times you’ll see the syntax affectionately known as the
> "turbofish", for example: [`Iterator::collect::<Vec<i32>>()`].
> ```
>
> `collect()` is one of the few times you’ll see the syntax affectionately known as the
> "turbofish", for example: [`Iterator::collect::<Vec<i32>>()`].

## Functions and macros

To indicate that an item is a function, add `()` after the function name. To indicate
that an item is a macro, add `!` after the macro name, optionally followed by `()`,
`[]`, or `{}`. This is _compatible_ with rustdoc.

Note that there cannot be arguments within `()`, `[]`, or `{}`.

> ```md
> [`vec!`][std::vec!][^2] is different from [`vec`][std::vec], and don't accidentally
> use [`format()`][std::fmt::format()] in place of [`format!()`][std::format!()][^2]!
> ```
>
> [`vec!`][std::vec!][^2] is different from [`vec`][std::vec], and don't accidentally
> use [`format()`][std::fmt::format()] in place of [`format!()`][std::format!()][^2]!

The macro syntax works for attribute and derive macros as well (even though this is not
how they are invoked).

> ```md
> There is a [derive macro][serde::Serialize!] to generate implementations of the
> [`Serialize`][serde::Serialize] trait.
> ```
>
> There is a [derive macro][serde::Serialize!] to generate implementations of the
> [`Serialize`][serde::Serialize] trait.

## Implementors and fully qualified syntax

Trait implementors may supply additional documentation about their implementations. To
link to implemented items instead of the traits themselves, use fully qualified paths,
including `<... as Trait>` if necessary. This is a _new feature_ that rustdoc does not
currently support.

> ```md
> [`Result<T, E>`] implements [`IntoIterator`]; its
> [**`into_iter()`**][Result::<(), ()>::into_iter] returns an iterator that yields one
> value if the result is [`Result::Ok`], otherwise none.
>
> [`Vec<T>`] also implements [`IntoIterator`]; a vector cannot be used after you call
> [**`into_iter()`**][<Vec<()> as IntoIterator>::into_iter].
> ```
>
> [`Result<T, E>`] implements [`IntoIterator`]; its
> [**`into_iter()`**][Result::<(), ()>::into_iter] returns an iterator that yields one
> value if the result is [`Result::Ok`], otherwise none.
>
> [`Vec<T>`] also implements [`IntoIterator`]; a vector cannot be used after you call
> [**`into_iter()`**][<Vec<()> as IntoIterator>::into_iter].

> [!NOTE]
>
> If your type has generic parameters, you must supply concrete types for them for
> rust-analyzer to be able to locate an implementation. That is, `Result<T, E>` won't
> work, but `Result<(), ()>` will (unless there happen to be types `T` and `E` literally
> in scope).

## Disambiguators

rustdoc's [disambiguator syntax][disambiguator] `prefix@name` is **accepted but
ignored**:

> ```md
> [`std::vec`], [`mod@std::vec`], and [`macro@std::vec`] all link to the `vec` _module_.
> ```
>
> [`std::vec`], [`mod@std::vec`], and [`macro@std::vec`] all link to the `vec` _module_.

Currently, duplicate names in Rust are allowed only if they correspond to items in
different [namespaces], for example, between macros and modules, and between struct
fields and methods — this is mostly covered by the function and macro syntax, described
[above](#functions-and-macros).

If you encounter items that must be disambiguated using rustdoc's disambiguator syntax,
other than [the "special types" listed below](#special-types), please [file an
issue][gh-issues]!

## Special types

> [!WARNING]

There is **no support** on types whose syntax is not a path; they are currently not
parsed at all:

> references `&T`, slices `[T]`, arrays `[T; N]`, tuples `(T1, T2)`, pointers like
> `*const T`, trait objects like `dyn Any`, and the never type `!`

Note that such types can still be used as generic params, just not as standalone types.

## Struct fields

> [!WARNING]

Linking to struct fields is **not supported** yet. This is **incompatible** with
rustdoc.

## Markdown link syntax

All Markdown link formats supported by rustdoc are supported:

Linking with URL inlined:

> ```md
> [The Option type](std::option::Option)
> ```
>
> [The Option type](std::option::Option)

Linking with reusable references:

> ```md
> [The Option type][option-type]
>
> [option-type]: std::option::Option
> ```
>
> [The Option type][option-type]
>
> [option-type]: std::option::Option

Reference-style links `[text][id]` without a corresponding `[id]: name` part will be
treated the same as inline-style links `[text](id)`:

> ```md
> [The Option type][std::option::Option]
> ```
>
> [The Option type][std::option::Option]

Shortcuts are supported, and can contain inline markups:

> ```md
> You can create a [`Vec`] with [**`Vec::new`**], or by using the [_`vec!`_][^2] macro.
> ```
>
> You can create a [`Vec`] with [**`Vec::new`**], or by using the [_`vec!`_][^2] macro.

(The items must still be resolvable; in this case `Vec` and `vec!` come from the
prelude.)

## Linking to page sections

To link to a known section on a page, use a URL fragment, just like a normal link. This
is _compatible_ with rustdoc.

<!-- prettier-ignore-start -->

> ```md
> [When Should You Use Which Collection?][std::collections#when-should-you-use-which-collection]
> ```
>
> [When Should You Use Which Collection?][std::collections#when-should-you-use-which-collection]

<!-- prettier-ignore-end -->

---

[^1]:
    rust-analyzer's ability to generate links for enum variants like `Option::Some` was
    improved only somewhat recently: before
    [#19246](https://github.com/rust-lang/rust-analyzer/pull/19246), links for variants
    and associated items may only point to the types themselves. If linking to such
    items doesn't seem to work for you, be sure to upgrade to a newer rust-analyzer
    first!

[^2]:
    As of rust-analyzer <ra-version>(version)</ra-version>, links generated for macros
    don't always work. Examples include [`std::format!`] (seen above) and
    [`tokio::main!`]. For more info, see [Known issues](known-issues.md#macros).

<!-- prettier-ignore-start -->

[disambiguator]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html#namespaces-and-disambiguators
[gh-issues]: https://github.com/tonywu6/mdbookkit/issues
[namespaces]: https://doc.rust-lang.org/reference/names/namespaces.html
[rust-types]: https://doc.rust-lang.org/reference/types.html#r-type.kinds
[rustdoc-linking]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html

<!-- prettier-ignore-end -->
