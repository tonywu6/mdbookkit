Control the preprocessor's logging output.

The same variable also controls logging output of the main `mdbook` executable.

The preprocessor uses the `tracing` family of crates for logging. See [its
documentation][EnvFilter#directives] for more details on how to customize this
environment variable.
