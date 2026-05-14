# Logging

With the environment variables `MDBOOK_LOG` and `CI`, you can control how the
preprocessor emits logs and diagnostic information.

## Output style

{{#include ../_snippets/logging/output-style.md}}

<figure>

![miette warnings](_media/error-reporting.png)

<figcaption>

Diagnostics are displayed in a graphical manner by default.

</figcaption>

</figure>

<figure>

![tracing logs](_media/diagnostics-tracing.png)

<figcaption>

The same diagnostics emitted as logs

</figcaption>

</figure>

## `MDBOOK_LOG`

{{#include ../_snippets/logging/env-var.md}}
