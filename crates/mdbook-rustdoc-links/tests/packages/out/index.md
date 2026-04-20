This link should resolve because the package is specified in `[build.packages]`:

[`unicode_ident::is_xid_start`](https://docs.rs/unicode-ident/1.0.24/unicode_ident/fn.is_xid_start.html "fn unicode_ident::is_xid_start")

This link should resolve because the package is a dependency of `anstyle-parse` which is
specified in `[build.packages]`:

[`utf8parse::Parser`](https://docs.rs/utf8parse/0.2.2/utf8parse/struct.Parser.html "struct utf8parse::Parser")

This link should not resolve because it is not specified in `[build.packages]`:

[`itoa`]

This link should not resolve because when `[build.packages]` is specified, the current
package is not implicitly added to the list, nor is it in the prelude:

[`crate::fun`]
