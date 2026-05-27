A ["preprocessor"] is just an executable that mdBook will run during builds to customize
the build process. You can build and install this preprocessor from source using
`cargo`:

```sh
cargo install {{ preprocessor }}
```

<p><details>
  <summary>Other ways to install</summary>

- This project supports [cargo-binstall], so instead of compiling from source, you can
  install a precompiled binary:

  ```sh
  cargo binstall {{ preprocessor }}
  ```

- You can also download binaries directly from [GitHub releases][gh-releases].

</details></p>

<!-- prettier-ignore-start -->
["preprocessor"]: https://rust-lang.github.io/mdBook/format/configuration/preprocessors.html
[cargo-binstall]: https://github.com/cargo-bins/cargo-binstall
[gh-releases]: https://github.com/tonywu6/mdbookkit/releases
<!-- prettier-ignore-end -->
