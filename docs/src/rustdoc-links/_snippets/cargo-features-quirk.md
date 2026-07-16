It is currently not possible to specify features in dependencies if you also use the
[`build.packages` option](/docs/src/rustdoc-links/reference/configuration.md#buildpackages)
but it _only_ selects dependencies. A workaround is to include at least one workspace
member in the `packages` option. See
[cargo issue#16990](https://github.com/rust-lang/cargo/issues/16990) for more
information.
