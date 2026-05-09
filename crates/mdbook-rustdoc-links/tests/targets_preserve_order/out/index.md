- [`amd64_only`](https://docs.rs/targets_preserve_order/0.1.0/x86_64-unknown-linux-gnu/targets_preserve_order/fn.amd64_only.html "fn targets_preserve_order::amd64_only")
- [`arm64_only`](https://docs.rs/targets_preserve_order/0.1.0/aarch64-unknown-linux-gnu/targets_preserve_order/fn.arm64_only.html "fn targets_preserve_order::arm64_only")
- [`universal`](https://docs.rs/targets_preserve_order/0.1.0/x86_64-unknown-linux-gnu/targets_preserve_order/fn.universal.html "fn targets_preserve_order::universal")
  - Earlier items in `build.targets` should have priority over later ones, so this link
    should be `x86_64-unknown-linux-gnu` even though it sorts after `aarch64`.
