The `MDBOOK_LOG` environment variable allows you to control the verbosity of logging
messages. The same variable also controls [mdBook's logging output][mdbook-init-logger].

The variable has semantics similar to the commonly seen `RUST_LOG` variable. For
example:

- `MDBOOK_LOG=info` enables logging at the `info` (regular) level. The preprocessor will
  emit the same amount of information, including diagnostics, as when the variable is
  not set, except graphical output styles are not used (see [above](#output-style)).

- `MDBOOK_LOG=debug` enables `debug` (more verbose) logging.

The variable is supported by `tracing-subscriber`. See
[`EnvFilter`][tracing_subscriber::filter::EnvFilter] for the complete usage info.

> [!NOTE]
>
> mdBook [v0.4.x uses the `RUST_LOG` variable][mdbook-init-logger-v0.4] instead of
> `MDBOOK_LOG`. This preprocessor supports both, with `MDBOOK_LOG` taking precedence.

<!-- prettier-ignore-start -->
[mdbook-init-logger]: https://github.com/rust-lang/mdBook/blob/v0.5.2/src/main.rs#L94-L97
[mdbook-init-logger-v0.4]: https://github.com/rust-lang/mdBook/blob/v0.4.52/src/main.rs#L111-L112
<!-- prettier-ignore-end -->
