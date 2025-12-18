<!-- TODO: -->

By default, the preprocessor shows a progress spinner when it is running.

When running in CI, progress is instead printed as logs (using [log] and
[env_logger])[^stderr].

You can control logging levels using the [`RUST_LOG`] environment variable.

[^stderr]:
    Specifically, when stderr is redirected to something that isn't a terminal, such as
    a file.

<!-- prettier-ignore-start -->

[`RUST_LOG`]: https://docs.rs/env_logger/latest/env_logger/#enabling-logging

<!-- prettier-ignore-end -->
