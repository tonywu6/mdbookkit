# Lints: Error handling

In your audit, look for the following anti-patterns.

## `unannotated_errors`

The error is allowed to be printed without program-specific context attached. This is
especially undesirable for errors from external APIs.

The following code prints `No such file or directory (os error 2)` when the file cannot
be found, with no extra explanations, which is unhelpful.

```rs
fn main() -> Result<()> {
    let data = std::fs::read_to_string(path)?;
}
```

## `terminating_errors`

The error is allowed to bubble to `main()` and cause the program to exit. Also
applicable are errors that alter control flows in possibly unintended ways, such as
causing a function to exit early while it is still in a loop.

While some errors are severe enough to cause an exit, many other errors are likely
non-fatal. Operations that interact with the environment (e.g. the filesystem) should
especially be robust. Frequent exits cause the software to be perceived as brittle.

In your report, _include your reasoning as to why the error should be
non-fatal/non-breaking instead._

The following code contains an oversight that causes the function/program to exit when
it cannot read from cache, which could be due to many reasons, but ideally cache misses
should be non-fatal.

```rs
let cache = read_from_cache()?;
```

## `suppressed_errors`

The error is dropped without being logged, such as due to a call to `ok()`,
`unwrap_or()`, or via pattern matching. This may be undesirable. Non-fatal errors, such
as an unparsable option that has sensible fallbacks, may still merit visibility so that
people may address the underlying cause. For errors that are assumed to be expected,
debug messages could be printed to aid with troubleshooting in case such assumptions
turn out to be false.

In your report, _include your reasoning as to how the error is significant enough to be
logged, and your suggestion of what the logging level should be._

In the following code, the error is dropped during pattern matching. While cache reading
errors may be expected, it is useful for them to be visible during debugging.

```rs
if let Some(cache) = read_from_cache() {}
```

## `double_logging`

The error is logged once and then logged a second time higher up in the call stack,
possibly with added context. This results in people seeing multiple messages with
repeating text. Usages of the `emit_*!` macros is especially prone to this issue when
they are not coordinated.

Although these rules are flexible, in general:

- Non-fatal warnings are emitted higher in the call stack to allow for more context;
- Debugging messages are emitted closer to where the error occurs to preserve their
  location in code;
- Errors in "atomic" functions are simply propagated; it is acceptable for them to be
  logged together at call site;
- It is acceptable for an error to be first printed as debug messages and then as
  warnings, since debug logs are hidden in normal usage.

In the following code, errors from `read_to_end` are printed as warnings twice. The
inner log can be demoted to a debug message, or omitted altogether.

```rs
fn main_task() -> Result<()> {
    let options = parse_options()
        .context("Could not parse options")
        .inspect_err(emit_warning!())?;
}
fn parse_options() -> Result<Options> {
    std::io::stdin().read_to_end(&mut buf)
        .inspect_err(emit_warning!())?;
}
```

## `insufficient_context`

This lint is largely advisory. There could be varying reasons why error context may be
perceived as "insufficient":

1. Major: Context messages provide only trivial, unhelpful, or unactionable details:

   In the following code, piecemeal context about each stage of the parsing logic is
   irrelevant.

   ```rs
   std::io::stdin().read_to_end(&mut buf)
       .context("Failed to read from stdin")?;
   let input = String::from_utf8(buf)
       .context("Failed to decode input as UTF-8")?;
   let options = serde_json::from_str::<Options>(&input)
       .context("Failed to parse program options")?;
   ```

   It would have been more helpful to group these together in a separate function, and
   explain what the program is trying to do in the error message:

   ```rs
   read_options()
       .context("You may be using an incompatible version of the program")
       .context("Error while parsing program options via stdin")?;
   ```

2. Minor: Context messages describe its immediate context, but after the entire error
   has been printed out, that context failed to explain how it is related to the purpose
   of the program at all.

   In the following code, while a context message has been supplied ...

   ```rs
   fn main() -> Result<()> {
       info!("Preprocessor started");
       cargo_subcommand()
           .context("Failed to determine the workspace root via cargo")?;
   }
   ```

   ... when printed out, it is not immediately accessible to a human reader why the
   "workspace root" is relevant:

   ```
   ERROR: Failed to determine the workspace root via cargo

   Caused by:
       1: (cargo stderr)
       2: command exited with ...
   ```

   The follow code adds a more gentle introduction to the situation:

   ```rs
   cargo_subcommand()
       .context("While trying to build the required documentation files")
       .context("A cargo subcommand did not succeed")?;
   ```

   Alternatively, it's [recommended](#spans_recommended) to introduce spans to easily
   scope log messages with common prefixes.

## `spans_recommended`

Contextual information is duplicated due to the operation involving multiple fallible
steps. This is similar to `unhelpful_context`, except in this case, introducing spans
would be useful.

In the following code, the `path` context is repeated throughout the parsing operation.

```rs
let input = read_from_file(path)
    .context(path)
    .context("Error reading from file")?;
let stream = Lexer::new(input)
    .context(path)
    .context("Error lexing the source code")?;
let ast = DeriveInput::parse(stream)
    .context(path)
    .context("Error parsing the item")?;
```

It would have been better to attach it as a span attribute, which will be shown for any
message emitted within the span:

```rs
#[instrument]
fn parse_derive(path: &str) {
    let input = read_from_file(path)
        .context("Error reading from file")
        .inspect_err(emit_debug!())?;
}
```

## `inconsistent_casing`

The error message does not follow the recommended letter casing.

In the following code, the error is emitted as warnings but uses lowercase.

```rs
read_from_file(path)
    .context("error reading from file")
    .inspect_err(emit_warning!())?;
```
