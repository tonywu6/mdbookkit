When running `cargo` commands with:

- `--package` flags only specifying both workspace members and dependencies, and a
- `--features` flag specifying features for the dependency,

then `cargo` doesn't fail with "cannot specify features for packages outside of
workspace".
