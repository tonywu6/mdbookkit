By default, the preprocessor reports progress using a spinner, and diagnostic
information (such as broken links) are displayed in a graphical manner, similar to how
rustc emits errors.

If `MDBOOK_LOG` is set, or if [`CI=true`](continuous-integration.md), then all messages
are emitted as logs instead.
