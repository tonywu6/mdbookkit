When running `cargo` commands with:

- `--package` flags only specifying packages that are not workspace members, and a
- `--features` flag specifying features for such packages,

then `cargo` always fails with "cannot specify features for packages outside of
workspace", despite the Cargo book suggesting that
[this should be possible](https://doc.rust-lang.org/stable/cargo/reference/features.html#resolver-version-2-command-line-flags).
