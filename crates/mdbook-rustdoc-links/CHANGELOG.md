# CHANGELOG

## 3.0.0

[Code changes from v2.0.1 to v3.0.0](https://github.com/tonywu6/mdbookkit/compare/mdbook-rustdoc-links-v2.0.1...mdbook-rustdoc-links-v3.0.0)

The preprocessor has been rewritten to **no longer depend on rust-analyzer!** Instead, it utilizes `cargo doc` directly.

- The preprocessor now requires **only a _stable_ Rust toolchain** (nightly not required).

  If you had previously setup your CI pipelines to run the preprocessor, you can safely remove rust-analyzer related instructions from your workflows.

- The preprocessor now parses and generates links **exactly the same way that [rustdoc](https://doc.rust-lang.org/rustdoc/what-is-rustdoc.html) does.**

  For example, you can now both use the [disambiguator syntax](https://docs.tonywu.dev/mdbookkit/rustdoc-links/writing-links#namespaces-and-disambiguators) and link to struct fields, where the previous version [did not support them](https://github.com/tonywu6/mdbookkit/blob/mdbook-rustdoc-links-v2.0.1/docs/src/rustdoc-links/supported-syntax.md#unsupported-syntax). There were also [subtle cases](https://github.com/tonywu6/mdbookkit/blob/mdbook-rustdoc-links-v2.0.1/docs/src/rustdoc-links/known-issues.md#incorrectunresolvable-links) in which the preprocessor would generate incorrect links.

  > In other words, if `cargo doc` can successfully resolve an intra-doc link in a doc comment, then the preprocessor is now expected to also resolve the same link in mdBook (so long as the item is ["in scope"](https://docs.tonywu.dev/mdbookkit/rustdoc-links/naming-items)). If you notice any discrepancies, please don't hesitate to [file an issue](https://github.com/tonywu6/mdbookkit/issues)!

- The preprocessor also inherits [rustdoc's **incredibly helpful diagnostics**](https://doc.rust-lang.org/stable/rustdoc/lints.html).

- The preprocessor now has the **same performance and caching capability as `cargo doc`**.

  The preprocessor no longer has to spawn `rust-analyzer` every time `mdbook serve` refreshes (which was noticeably slow and resource-hungry even for moderately sized projects), or use a bespoke caching mechanism.

- Because the preprocessor essentially automates running `cargo doc`, when authoring documentation, you can choose to [**preview the local version of your crate docs**](https://docs.tonywu.dev/mdbookkit/rustdoc-links/how-to/local-development), instead of what's on docs.rs. You can even choose to bundle your crate docs with your book, if you are looking to [self-host](https://docs.tonywu.dev/mdbookkit/rustdoc-links/how-to/self-hosting-cargo-docs) them.

To learn more about the new version, feel free to peruse the updated [tutorials](https://docs.tonywu.dev/mdbookkit/rustdoc-links/getting-started) and [how-to guides](https://docs.tonywu.dev/mdbookkit/rustdoc-links/how-to)!

### Breaking changes

- The following options have been removed.

  - `rust-analyzer`
  - `rust-analyzer-timeout`
  - `cache-dir`
  - `cargo-features` (superseded by a new [build config format](https://docs.tonywu.dev/mdbookkit/rustdoc-links/how-to/conditional-compilation#specifying-features))

- The preprocessor now **by default only processes items from packages in your workspace** (as opposed to e.g. items from dependencies).

  You can document more packages, including dependencies, by configuring the [`build.packages`](https://docs.tonywu.dev/mdbookkit/rustdoc-links/how-to/package-selection) option. This was previously not configurable, and rust-analyzer would need to index every package used, even if the majority of them you didn't need in your documentation.

- The ["standalone usage"](https://github.com/tonywu6/mdbookkit/blob/mdbook-rustdoc-links-v2.0.1/docs/src/rustdoc-links/standalone-usage.md) mode has been removed.

- The "link report" feature is now controlled by a [dedicated environment variable](https://docs.tonywu.dev/mdbookkit/rustdoc-links/reference/environment-variables#mdbookkit_link_report).

## 2.0.1

[Code changes from v2.0.0 to v2.0.1](https://github.com/tonywu6/mdbookkit/compare/mdbook-rustdoc-links-v2.0.0...mdbook-rustdoc-links-v2.0.1)

### <!-- 1 --> Fixed

- Ignore the new [`optional` preprocessor option](https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html#optional-preprocessors) when deserializing `book.toml` [`52e54ec`](https://github.com/tonywu6/mdbookkit/commit/52e54ec23ce40a90a065956d9d784298d7507fd8)

## 2.0.0

[Code changes from v1.1.2 to v2.0.0](https://github.com/tonywu6/mdbookkit/compare/mdbookkit-v1.1.2...mdbook-rustdoc-links-v2.0.0)

`mdbook-rustdoc-links` is now a standalone package. To install the new version, use:

```sh
cargo install mdbook-rustdoc-links
```

If you previously installed via the `mdbookkit` package, you should remove the old binary:

```sh
cargo uninstall mdbookkit
```

Note that the executable name has changed. You should also update the table name in `book.toml`:

```diff
- [preprocessor.rustdoc-link]
+ [preprocessor.rustdoc-links]
```

### Added

- Added a "link report" logging feature that prints a summary of all generated links upon build.

- The indexing process now presents more progress updates from rust-analyzer and should feel more responsive.

- The preprocessor will emit a warning regarding caches if there are any broken links. Broken links cause rust-analyzer to always run since they are always considered unseen.

- Diagnostic messages, such as for broken links, now [include source locations (file:line:column) when the preprocessor is in logging mode](https://docs.tonywu.dev/mdbookkit/rustdoc-links/reference/environment-variables#mdbookkit_term_graphical).

### Changed

- **mdBook 0.5 is now supported.** See the [official migration guide][mdbook-0.5] for more details.
  - mdBook 0.4 is now unsupported, although as of mdBook 0.5.2, the preprocessor can still run under mdBook 0.4. There is no guarantee that it will remain compatible in the future.

- `MDBOOK_LOG` is now the environment variable to control logging. This variable also controls logging in the main `mdbook` program. Previously, the variable was ~~`RUST_LOG`~~.
  - Logging is now implemented through `tracing`. The `MDBOOK_LOG` variable therefore supports [all syntax supported by `tracing`][tracing_subscriber::filter::EnvFilter]. See [Logging](https://docs.tonywu.dev/mdbookkit/rustdoc-links/reference/environment-variables#mdbook_log) for more information.

- **\[BREAKING\]** The `book.toml` config table for this preprocessor is now `[preprocessor.rustdoc-links]`.

- **\[BREAKING\]** The `smart-punctuation` option has been removed. It has no meaningful effect on this preprocessor.

### Documentation

- Added a dedicated ["Logging"](https://github.com/tonywu6/mdbookkit/blob/mdbook-rustdoc-links-v2.0.1/docs/src/rustdoc-links/logging.md) page.

<!-- prettier-ignore-start -->
[mdbook-0.5]: https://github.com/rust-lang/mdBook/blob/master/CHANGELOG.md#05-migration-guide
<!-- prettier-ignore-end -->
