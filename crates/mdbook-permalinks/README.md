# mdbook-permalinks

An [mdBook] [preprocessor] that lets you link to files in your Git repository using
paths instead of hard-coded URLs.

You simply write ...

```md
Here is a link to the project's [Cargo.toml](../Cargo.toml).
```

... and the preprocessor will convert the link to a versioned permalink during build.
Supports GitHub or
[your Git forge of choice](https://docs.tonywu.dev/mdbookkit/permalinks/configuration#repo-url-template).

## To see it in action, [read the book!][book]

<!-- prettier-ignore-start -->

**Quick access**
| [Install](https://docs.tonywu.dev/mdbookkit/permalinks/getting-started#install)
| [Quickstart](https://docs.tonywu.dev/mdbookkit/permalinks/getting-started#configure)
| [Features](https://docs.tonywu.dev/mdbookkit/permalinks/features)
| [Options](https://docs.tonywu.dev/mdbookkit/permalinks/configuration)

<!-- prettier-ignore-end -->

```sh
cargo install mdbook-permalinks
```

<!-- prettier-ignore-start -->
[mdBook]: https://rust-lang.github.io/mdBook/
[preprocessor]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[book]: https://docs.tonywu.dev/mdbookkit/permalinks
<!-- prettier-ignore-end -->
