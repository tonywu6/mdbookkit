- [`amd64_only`]
- [`arm64_only`]
- [`universal`]
  - Earlier items in `build.targets` should have priority over later ones, so this link
    should be `x86_64-unknown-linux-gnu` even though it sorts after `aarch64`.
