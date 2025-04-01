# Known issues

## Working with `{{#include}}`

Linking by relative paths may not make sense when the links are in files that are
embedded using mdBook's `{{#include}}` directive.

See [Working with `{{#include}}`](working-with-include.md) for some possible
workarounds.

## Links in HTML

Links in HTML (`href` and `src`) are currently neither transformed nor checked.
