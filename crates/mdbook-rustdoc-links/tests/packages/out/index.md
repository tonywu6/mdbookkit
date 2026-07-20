These links should resolve because the package is specified in `[build.packages]`:

- [`tap`](https://docs.rs/tap/1.0.1/tap/index.html "mod tap")
- [`tap::Tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html "trait tap::tap::Tap")
- [`tap::Tap::tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html#method.tap "method tap::tap::Tap::tap")
- [`::tap::Tap::tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html#method.tap "method tap::tap::Tap::tap")

These links should not resolve because they are spelled wrong:

- [`tap::tap::Tab`]
- [`tap::tap::Tap::tab`]

This link should resolve because the package is a dependency of `anstyle-parse` which is
specified in `[build.packages]`:

- [`utf8parse::Parser`](https://docs.rs/utf8parse/0.2.2/utf8parse/struct.Parser.html "struct utf8parse::Parser")

This link should not resolve because it is not specified in `[build.packages]`:

- [`pin_project_lite`]

This link should not resolve because when `[build.packages]` is specified, the current
package is not implicitly added to the list, nor is it in the prelude:

- [`crate::fun`]
