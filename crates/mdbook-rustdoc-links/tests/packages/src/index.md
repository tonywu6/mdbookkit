This link should resolve because the package is specified in `[build.packages]`:

[`utf8_width::get_width`]

This link should resolve because the package is a dependency of `anstyle-parse` which is
specified in `[build.packages]`:

[`utf8parse::Parser`]

This link should not resolve because it is not specified in `[build.packages]`:

[`pin_project_lite`]

This link should not resolve because when `[build.packages]` is specified, the current
package is not implicitly added to the list, nor is it in the prelude:

[`crate::fun`]
