# Known issues

## Working with `{{#include}}`

Linking by relative paths may not work when the links are in files that are embedded
using mdBook's `{{#include}}` directive.

See [More ways to link](more-ways-to-link.md) for some possible workarounds.

## Links in HTML

URLs in HTML (`href` and `src`) are currently unsupported. They are neither checked nor
transformed.
