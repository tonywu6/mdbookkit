# Supported syntax

This page lists all syntax supported by the `mdbook-rustdoc-link`.

Most of the formats [supported by rustdoc][rustdoc-linking] are supported. Unsupported
syntax and differences in behavior are emphasized below.

[rustdoc-linking]:
  https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html

In general, specifying items should "just work" as you would when writing Rust code.

## Types, modules, and associated items

> ```md
> Module [`alloc`][std::alloc] — Memory allocation APIs.
> ```
>
> Module [`alloc`][std::alloc] — Memory allocation APIs.
>
> ```md
> Type [`Option`] represents an optional value: every [`Option`] is either
> [`Some`][Option::Some] and contains a value, or [`None`][Option::None], and does not.
> ```
>
> Type [`Option`] represents an optional value: every [`Option`] is either
> [`Some`][Option::Some] and contains a value, or [`None`][Option::None], and does not.
>
> ```md
> [`Ipv4Addr::LOCALHOST`][core::net::Ipv4Addr::LOCALHOST] — An IPv4 address with the
> address pointing to localhost: `127.0.0.1`.
> ```
>
> [`Ipv4Addr::LOCALHOST`][core::net::Ipv4Addr::LOCALHOST] — An IPv4 address with the
> address pointing to localhost: `127.0.0.1`.

## Generic parameters

Types can contain generic parameters. This is compatible with rustdoc.

> ```md
> [`Vec<T>`] — A heap-allocated _vector_ that is resizable at runtime.
> ```
>
> [`Vec<T>`] — A heap-allocated _vector_ that is resizable at runtime.
>
> ```md
> | Phantom type                                       | variance of `T`   |
> | :------------------------------------------------- | :---------------- |
> | [`&'a mut T`][std::marker::PhantomData<&'a mut T>] | **inv**ariant     |
> | [`fn(T)`][std::marker::PhantomData<fn(T)>]         | **contra**variant |
> ```
>
> | Phantom type                                       | variance of `T`   |
> | :------------------------------------------------- | :---------------- |
> | [`&'a mut T`][std::marker::PhantomData<&'a mut T>] | **inv**ariant     |
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
that an item is a macro, add `!` after the macro name, which can be optionally followed
by `()`, `[]`, or `{}`. This is compatible with rustdoc.

Note that there cannot be arguments within `()`, `[]`, or `{}`.

> ```md
> [`vec!`][std::vec!] is different from [`vec`][std::vec], and don't accidentally use
> [`format()`][std::fmt::format()] in place of [`format!()`][std::format!()]!
> ```
>
> [`vec!`][std::vec!] is different from [`vec`][std::vec], and don't accidentally use
> [`format()`][std::fmt::format()] in place of [`format!()`][std::format!()]!

The macro syntax works for attribute and derive macros as well, even though this is not
how they are invoked.

> ```md
> There is a [derive macro][serde::Serialize!] to generate implementations of the
> [`Serialize`][serde::Serialize] trait.
> ```
>
> There is a [derive macro][serde::Serialize!] to generate implementations of the
> [`Serialize`][serde::Serialize] trait.

> [!WARNING]
>
> As of `rust-analyzer 2025-03-10`, links generated for re-exported items don't always
> work. This is because rust-analyzer resolves items to the modules that define them,
> but docs for the source modules may not be have been published.
>
> This happens often with macros. Examples include [`std::format!`] (seen above) and
> [`tokio::main!`].

## Implementors and fully qualified syntax

## Disambiguators

## Special types

## Markdown link syntax

All Markdown link formats supported by rustdoc are supported:

**Shortcut links**:

> ```md
> [Vec] or [Vec][]
> ```
>
> [Vec] or [Vec][]

**Reference links**:

> ```md
> [A contiguous growable array type][Vec]
>
> [The Option type][Option]
>
> [Option]: std::option::Option
> ```
>
> [A contiguous growable array type][Vec]
>
> [The Option type][Option]
>
> [Option]: std::option::Option

**Inline markups** are supported within shortcut links:

> ```md
> You can explicitly create a [`Vec`] with [**`Vec::new`**], or by using the [_`vec!`_]
> macro.
> ```
>
> You can explicitly create a [`Vec`] with [**`Vec::new`**], or by using the [_`vec!`_]
> macro.
