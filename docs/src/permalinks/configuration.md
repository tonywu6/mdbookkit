# Configuration

This page lists all options for the preprocessor.

Configure options in the `[preprocessor.permalinks]` table using the keys below, for
example:

```toml
[preprocessor.permalinks]
always-link = [".rs"]
```

<permalinks-options>(auto-generated)</permalinks-options>

## `output.html.git-repository-url`

> [!NOTE]
>
> This option is configured under [mdBook's `[output.html]`
> table][html-renderer-options].

If configured, this URL will be the prefix of the generated permalinks.

If not configured, the preprocessor will attempt to retrieve a URL from the Git remote
with the name `origin`.

<!-- prettier-ignore-start -->
[html-renderer-options]: https://rust-lang.github.io/mdBook/format/configuration/renderers.html#html-renderer-options
<!-- prettier-ignore-end -->
