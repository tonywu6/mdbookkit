# CHANGELOG

## 3.0.0

[Code changes from v2.0.1 to v3.0.0](https://github.com/tonywu6/mdbookkit/compare/mdbook-permalinks-v2.0.1..book-permalinks-v3.0.0)

### Upgrading from v2

Although this is a major version increase, no extra migration steps are required when upgrading! Version 3 of the preprocessor remains largely compatible with the previous version.

### <!-- 0 --> Added

- **More Git forges:** The preprocessor now has built-in support for repositories hosted on Tangled (<https://tangled.org>) and Codeberg (<https://codeberg.org>), in addition to GitHub.

  If your repo is on one of these sites, simply configuring the [`output.html.git-repository-url`](https://docs.tonywu.dev/mdbookkit/permalinks/how-to/remote-url#setting-git-repository-url) option or [setting up `git remote`](https://docs.tonywu.dev/mdbookkit/permalinks/how-to/remote-url#configuring-git-remote) will suffice, and the use of the [`repo-url-template`](https://docs.tonywu.dev/mdbookkit/permalinks/reference/configuration#repo-url-template) option is no longer necessary.

- **HTML support:** The preprocessor can now process [hyperlinks found in HTML markups](https://docs.tonywu.dev/mdbookkit/permalinks/getting-started#html-links).

  For example, you can now use [`<img>`](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/img) and [`<video>`](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/video) elements to include images and videos in your book, and the preprocessor will convert the URLs to permalinks.

- **Better local previewing:** An _experimental_ "dev mode," in which the preprocessor renders links that can be viewed locally instead of regular permalinks. This should provide a better editing experience when e.g. using `mdbook serve`. To learn more, see the [local development guide](https://docs.tonywu.dev/mdbookkit/permalinks/how-to/local-development).

- The `repo-url-template` option supports [additional customization](https://docs.tonywu.dev/mdbookkit/permalinks/reference/configuration#repo-url-templateparams--repo-url-templatetemplate).

### <!-- 1 --> Fixed

- The preprocessor now performs stricter validation, especially for links to book pages. It is now also aware of symlinks. As such, broken links that were previously accepted as valid may now have associated warnings. You may read more about the special cases in the [reference](https://docs.tonywu.dev/mdbookkit/permalinks/reference/behaviors).

### <!-- 2 --> Changed

- **The `book-url` option has been soft-deprecated.** Instead, you can simply use mdBook's `output.html.site-url` option and specify the same value.

  The `book-url` option previously enabled the preprocessor to validate hard-coded links to your book's website. Such validation is still supported because the preprocessor can now reuse the value of `output.html.site-url`. For more info, see the relevant section in the [URL checking guide](https://docs.tonywu.dev/mdbookkit/permalinks/how-to/hardcoded-links#checking-urls-to-your-book).

- When running in an empty Git repository (one without any commit), the preprocessor will no longer fail with an error, but will instead emit a warning.

- The preprocessor now provides improved diagnostic messages for broken links that should better explain how the link was determined to be incorrect.

## 2.0.1

[Code changes from v2.0.0 to v2.0.1](https://github.com/tonywu6/mdbookkit/compare/mdbook-permalinks-v2.0.0..book-permalinks-v2.0.1)

### <!-- 1 --> Fixed

- Ignore the new [`optional` preprocessor option](https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html#optional-preprocessors) when deserializing `book.toml` [`52e54ec`](https://github.com/tonywu6/mdbookkit/commit/52e54ec23ce40a90a065956d9d784298d7507fd8)

## 2.0.0

[Code changes from v1.1.2 to v2.0.0](https://github.com/tonywu6/mdbookkit/compare/mdbookkit-v1.1.2..book-permalinks-v2.0.0)

`mdbook-permalinks` is now a standalone package. To install the new version, use:

```sh
cargo install mdbook-permalinks
```

If you previously installed via the `mdbookkit` package, you should remove the old binary:

```sh
cargo uninstall mdbookkit
```

Note that the executable name has changed. You should also update the table name in `book.toml`:

```diff
- [preprocessor.link-forever]
+ [preprocessor.permalinks]
```

### Added

- The preprocessor now processes URLs that point to the `HEAD` of your repo. You can read more about [why such URLs instead of paths may be desirable](https://docs.tonywu.dev/mdbookkit/permalinks/how-to/hardcoded-links) in the documentation.

- The preprocessor now warns about links pointing to files/directories that are gitignored.

### Fixed

- Path-based links that are used in Markdown images are now [correctly converted to `raw` URLs instead of `tree` URLs](https://docs.tonywu.dev/mdbookkit/permalinks/getting-started#images).

### Changed

- **mdBook 0.5 is now supported.** See the [official migration guide][mdbook-0.5] for more details.
  - mdBook 0.4 is now unsupported, although as of mdBook 0.5.2, the preprocessor can still run under mdBook 0.4. There is no guarantee that it will remain compatible in the future.

- `MDBOOK_LOG` is now the environment variable to control logging. This variable also controls logging in the main `mdbook` program. Previously, the variable was ~~`RUST_LOG`~~.
  - Logging is now implemented through `tracing`. The `MDBOOK_LOG` variable therefore supports [all syntax supported by `tracing`][tracing_subscriber::filter::EnvFilter]. See [Logging](https://docs.tonywu.dev/mdbookkit/permalinks/reference/environment-variables#mdbook_log) for more information.

- **\[BREAKING\]** The `book.toml` config table for this preprocessor is now `[preprocessor.permalinks]`.

### Documentation

- Added a ["More ways to link"](https://github.com/tonywu6/mdbookkit/blob/mdbook-permalinks-v2.0.1/docs/src/permalinks/more-ways-to-link.md) page that clarifies the different types of linking supported by this preprocessor, as well as when to use which.

- Added a dedicated ["Logging"](https://github.com/tonywu6/mdbookkit/blob/mdbook-permalinks-v2.0.1/docs/src/permalinks/logging.md) page.

<!-- prettier-ignore-start -->
[mdbook-0.5]: https://github.com/rust-lang/mdBook/blob/master/CHANGELOG.md#05-migration-guide
<!-- prettier-ignore-end -->
