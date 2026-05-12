# How to use the preprocessor in a workspace

How the preprocessor functions in a [Cargo workspace][workspace] is largely no
differently than when you use it with a single package.

There is however a subtle difference: When using the preprocessor to document a single
library package, the preprocessor has a default configuration that allows you to use the
`crate::*` notation to refer to items in your library.

If your workspace has [multiple default members][default-member] that are also
libraries, then this default configuration is not applied, because it would be ambiguous
which "crate" you are referring to.

If you still would like to use the `crate::*` notation to refer to items in one of the
packages, then you can use the `build.preludes` option to introduce them into scope. For
example:

```toml config-example
[preprocessor.rustdoc-links]
build.preludes = ["my_library::*"]
```

This acts as if a `use my_library::*;` statement had been added, allowing you to use
e.g. `[crate::item]` to refer to `[my_library::item]`.

You can add any items that you wish, so long as each item results in a valid `use`
statement, for example:

```toml config-example
[preprocessor.rustdoc-links]
build.preludes = ["std::io::{self, *}", "std::sync::Arc"]
```

> [!TIP]
>
> To read more about the logic behind this, please see
> [Naming items](../naming-items.md).

<!-- prettier-ignore-start -->
[workspace]: https://doc.rust-lang.org/cargo/reference/workspaces.html
[default-member]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field
<!-- prettier-ignore-end -->
