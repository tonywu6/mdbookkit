# Caching

`mdbook-rustdoc-link` spawns a fresh `rust-analyzer` server every time it is run, by
default. `rust-analyzer` then reindexes your entire project before resolving links.

This significantly impacts the responsiveness of `mdbook serve` — it is as if for every
live reload, you had to reopen your editor, and it gets even worse the more dependencies
your project has.

To mitigate this, there is an experimental caching feature, disabled by default.

<details class="toc" open>
  <summary>Sections</summary>

- [Enabling caching](#enabling-caching)
- [How it works](#how-it-works)
- [Help wanted 🙌](#help-wanted-)
  - [Cache priming and progress tracking](#cache-priming-and-progress-tracking)
  - [Using `ra-multiplex`](#using-ra-multiplex)
  - [Thoughts](#thoughts)

</details>

## Enabling caching

In your `book.toml`, in the `[preprocessor.rustdoc-link]` table, set
[`cache-dir`](configuration.md#cache-dir) to the relative path of a directory of your
choice (_other than_ your book's `build-dir`), for example:

```toml
[preprocessor.rustdoc-link]
cache-dir = "cache"
# You can also point to an arbitrary directory in target/
```

Now, when `mdbook` rebuilds your book during `build` or `serve`, if your edit does not
involve changes in the set of Rust items to be linked, that is, no new items unseen in
the previous build, then the preprocessor reuses the previous resolution and **skips
rust-analyzer entirely**.

> [!IMPORTANT]
>
> If you use a directory under your book root directory, **make sure to also have a
> `.gitignore` in your book root dir to exclude it from source control**, or the cache
> file could trigger additional reloads. See [Specify exclude
> patterns][specify-exclude-patterns] in the mdBook documentation.
>
> **Do not** use your book's `build-dir` as the `cache-dir`: `mdbook` clears the output
> directory on every build, making this setup useless.

## How it works

The effectiveness of this mechanism is based on the following assumptions:

- Most of the changes made during authoring don't actually involve item links.
- Assuming the environment is unchanged, the same set of items should resolve to the
  same set of links.

The cache keeps the following information in a `cache.json`:

- The set of items to be resolved, and their resolved links
- The environment, as a checksum over the contents of:
  - Your crate's `Cargo.toml`
  - If you are using a workspace, the workspace's `Cargo.toml`
  - The entrypoint (`lib.rs` or `main.rs`)
  - For each item that is defined within your crate or workspace, its source file
  - (Note that `Cargo.lock` is currently not considered, nor are dependencies or `std`)

If a subsequent run has the same set of items (or a subset) and the same checksum
(meaning you did not update your code), then the preprocessor simply reuses the previous
results.

> [!TIP]
>
> Items that fail to resolve are not included in the cache.
>
> If you keep such broken links in your Markdown source, the cache will permanently
> miss, and rust-analyzer will run on every edit.

## Help wanted 🙌

The cache feature, as it currently stands, is a workaround at best. If you have insights
on how performance could be further improved, please [open an issue!][gh-issues].

### Cache priming and progress tracking

The preprocessor spawns rust-analyzer with [cache priming][ra-cache-priming] enabled
which contributes to the majority of build time.

Furthermore, the preprocessor relies on the LSP [Work Done
Progress][lsp-work-done-progress] notifications to know when rust-analyzer has finished
cache priming before actually sending out external docs requests. Because rust-analyzer
seems to automatically reindex multiple times, a "cooldown" mechanism was implemented,
to wait for a hard-coded 500ms after rust-analyzer reports indexing done, before
continuing.

Not doing either of these things causes many links to fail to resolve.

**Questions**:

- Is it possible to do it without cache priming?
- Is there a better way to track rust-analyzer's "readiness" without having to arbitrary
  sleep?

### Using `ra-multiplex`

[`ra-multiplex`] "allows multiple LSP clients (editor windows) to share a single
`rust-analyzer` instance per cargo workspace."

In theory, in an IDE setting (e.g. with VS Code), one could setup the IDE and
`mdbook-rustdoc-link` to both connect to the same `ra-multiplex` server. Then the
preprocessor doesn't need to wait for cache priming (the cache is already warm from IDE
use). Changes in the workspace could also be reflected in subsequent builds without the
preprocessor being aware of them (because the IDE is doing the synchronizing).

In reality, with the current version, connecting the preprocessor to `ra-multiplex`
seems to result in buggy builds. The initial build emits in many warnings despite all
items eventually resolving. Subsequent builds hang indefinitely before timing out.

**Question**:

- Is it possible to use `ra-multiplex` here?

### Thoughts

`mdbook` encourages a stateless architecture for preprocessors. Preprocessors are
expected to work like pure functions over the entire book, even for `mdbook serve`.
Preprocessors are not informed on whether they are invoked as part of `mdbook build`
(prefer fresh starts) or `mdbook serve` (maintain states between run).

`rust-analyzer`, meanwhile, has a stateful architecture that also doesn't yet have
[persistent caching][ra-persistent-cache][^1]. It is [designed][ra-architecture] to take
in a ground state (your project initially) and then evolve the state (your project
edited) entirely in memory.

So `rust-analyzer` has an extremely incremental architecture, perfect for complex
languages like Rust, and `mdbook` has an explicitly non-incremental architecture,
perfect for rendering Markdown. This makes them somewhat challenging to work well
together in a live-reload scenario.

> [!NOTE]
>
> The above is entirely my understanding of how the two projects work, which may have
> gross misconceptions or misuse of words. [Feedback of any kind is very welcome
> :)][gh-issues]

---

[^1]:
    It was mentioned that the [recently updated, salsa-ified rust-analyzer][salsa]
    (version `2025-03-17`) will unblock work on persistent caching, among many other
    things, so hopefully bigger changes are coming!

<!-- prettier-ignore-start -->

[gh-issues]: https://github.com/tonywu6/mdbookkit/issues
[lsp-work-done-progress]: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#workDoneProgress
[ra-architecture]: https://rust-analyzer.github.io/book/contributing/architecture.html#:~:text=The%20analyzer%20keeps%20all%20this%20input%20data%20in%20memory%20and%20never%20does%20any%20IO.
[ra-cache-priming]: https://rust-analyzer.github.io/book/configuration.html?highlight=cache%20priming#configuration
[ra-persistent-cache]: https://github.com/rust-lang/rust-analyzer/issues/4712
[`ra-multiplex`]: https://github.com/pr2502/ra-multiplex
[salsa]: https://rust-analyzer.github.io/thisweek/2025/03/17/changelog-277.html
[specify-exclude-patterns]: https://rust-lang.github.io/mdBook/cli/serve.html#specify-exclude-patterns

<!-- prettier-ignore-end -->
