During compilation, `cargo doc` must compile proc-macro dependencies. `rustdoc` also
requires loading proc-macro dependencies for downstream crates to load.

Unlike regular dependencies, proc-macro dependencies are always compiled in the host
target, regardless of the actual build target. That is, running
`cargo doc --target x86_64-unknown-linux-gnu` on `aarch64-apple-darwin` still compiles
proc-macro crates to `aarch64-apple-darwin`.

This test asserts that even when targets are specified, the preprocessor still captures
and passes the correct `-L` flags to `rustdoc`.

- [`cats::shelter`](https://docs.rs/cats/0.1.0/aarch64-unknown-linux-gnu/cats/fn.shelter.html "fn cats::shelter")
