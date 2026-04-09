---
name: audit-error-handling
description:
  Audit the UI/UX effectiveness of error reporting in Rust CLI programs and generate a
  report for further code improvements.
disable-model-invocation: true
license: CC-BY-4.0
---

## Motivation

It is important for programs in this project to provide error messages that balance the
following properties:

- **Contextual**: Messages provide sufficient clues to help people understand why an
  error occurs: what task is program is trying to finish, what resources are being
  accessed, etc;
- **Concise**: Messages are not overflowing with verbose details and are not inundated
  among other program output; this requires coordination between different parts of the
  program;
- **Actionable**: Whenever possible, provide direct suggestions as to what someone could
  do to fix the issue.

Secondarily, for project developers, error reporting should be:

- **Timely**: report issues at soon as they are encountered to preserve debugging
  context (e.g. module/function names and line/column numbers).

## Infrastructure

### Primitives

This project uses the following libraries:

- `anyhow`: for error propagation and attaching context;

- `tracing`: for message printing (`warning!`, etc.) and attaching context via spans
  (`info_span!` and `#[instrument]`, etc.).

### Strategies

Understand the different error handling strategies in this project:

- **Recoverable errors.** The majority of errors belong to this class. Often emitted as
  warnings, these are problems that deserve attention but is otherwise workable, and
  therefore do not interrupt the program.

- **Fatal errors,** for situations where it is not possible for the program to continue
  working. Fatal errors propagate to the `main()` function, where it is printed and the
  program exits with a non-zero status. Panics are not used for halting in this project
  except for circumstances considered unreachable.

- **Debug errors.** These aren't visible in normal usage but can be revealed by
  configuring `tracing-subscriber` via environment variables to help with
  troubleshooting. They are often errors from external APIs, and are often expected
  (e.g. `std::io::Error`s).

## Task

Your task is to walk through the program's source code and document every site where an
error may be present:

1. Begin from the program's `main()` function. Drill down on each function call. **Make
   an effort to follow imports, and read the corresponding source file.**

2. Look for sites possibly requiring error handling:
   1. The `?` operator, which indicates the error being propagated;
   2. `ok()`, `unwrap_or()` and variants, and pattern matching on `Result`s (including
      if-let and let-else): these suggest errors possibly being dropped;
   3. The use of `anyhow` and `tracing` primitives.

3. In your report, document each site by repeating the
   [provided template](references/template.md). Address
   [specific lints](references/lints.md), although you are free to add additional notes
   and observations not covered by lints.

4. **Write your report to a Markdown file** at the root of the repo with the name
   `error-handling-audit.<yyyymmdd>-<hhmmss>.md` (e.g.
   `error-handling-audit.20260101-161234.md`). It is recommended to write to the file
   once per each item or each source file.

5. [**Review your report**](#review)

## Patterns

Understand the reusable error-handling patterns in this project. This section provides
context to the list of [specific lints (anti-patterns)](#lints) that you will address in
the report.

### `.context()`

Use `anyhow`'s `context()`/`with_context()` functions to add descriptions to errors.

When to use: Especially relevant for generic errors coming from external APIs (e.g.
`serde`, `std::fs`, and subcommands).

```rs
let repo = match find_git_remote(&repo, book)
    .context("Error while finding a git remote URL")?
```

### `emit_warning!()`

`emit_warning!` and its variants are convenience macros that expands to a closure
`|e| ::tracing::warning!("{:?}", e)`. They are tailored for "tapping", such as via the
`.inspect_err()` method.

When to use: For surfacing non-fatal/debug errors as soon as they are encountered.

In the following code, an `Err(..)` is not fatal, but we are interested in the error
details produced by `describe()` (`git describe`) when troubleshooting.

```rs
if let Ok(tag) = head.as_object().describe()
    .inspect_err(emit_debug!("no exact tag found: {}"))
```

> Why macros: details such as module names and file locations in tracing events are
> statically generated. Using macros this way allows messages to precisely report their
> origins. If these were implemented as helper methods, error messages will all point to
> the helper instead of their actual locations.

### `.ok()`

Use `Result::ok` to indicate (and then drop) a non-fatal error, often immediately after
logging it (e.g. via `emit_warning!`).

```rs
std::fs::create_dir_all(self.path.directory())
    .context("Could not create intermediate directories")
    .inspect_err(emit_warning!())
    .ok();
```

### `tracing::Span`

Use `Span`s (via `span!` and `#[instrument]` macros) to add structured context to
messages.

When to use:

- For warnings and errors: provide concise context (via span labels) to messages,
  clarifying the stage the program is in when the issue occurs, especially relevant for
  long-running subroutines.
- For debugging messages: scope log entries with common prefixes and propagate important
  task parameters to subroutines (without duplicating them wherever logging is needed).

For example, the following code makes use of `#[instrument]` to both provide a label and
an attribute across subroutines:

```rs
#[instrument]
fn parse_document(path: &str) {
    tokens_to_ast();
}
fn tokens_to_ast() {
    warn!("File contains syntax issues");
}
```

```
WARN parse_document: File contains syntax issues path="path/to/file.md"
```

### "Atomic" functions

While contradictory at first, errors do not always need context before being propagated.
Consider the following:

```rs
fn main_task() -> Result<()> {
    let mut buf = vec![];
    std::io::stdin().read_to_end(&mut buf)
        .context("Failed to read from stdin")?;
    let input = String::from_utf8(buf)
        .context("Failed to decode input as UTF-8")?;
    let options = serde_json::from_str::<Options>(&input)
        .context("Failed to parse program options")?;
}
```

Not only are such context messages tedious for developers to write, they do not provide
clarity as to what the program actually is trying to do when the problem occurs.

Instead, it is recommended to abstract these operations in an "atomic" function, _spare
the unnecessary context messages,_ and provide one at the top instead:

```rs
fn main_task() -> Result<()> {
    let options = parse_options()
        .context("Could not read program options from input")?;
}
fn parse_options() -> Result<Options> {
    let mut buf = vec![];
    std::io::stdin().read_to_end(&mut buf)?;
    let input = String::from_utf8(buf)?;
    Ok(serde_json::from_str(&input)?)
}
```

### Case convention

- For messages with level `INFO` and more severe, use _Sentence case_ (which is
  consistent with other messages provided by upstream programs).
- For messages with level `DEBUG` and less severe, use _lowercase_ (which is easier for
  developers to write).

## Lints

Reference [the list of lints](references/lints.md) in your report.

## Review

Once you have finished writing, **read back your report** and check for any formatting
and logical errors.

**Important things to review:**

- Names in call stacks must have **fully-qualified module paths**. This includes
  **1st-party functions and symbols**

- **Skip extra explanations when there are no issues.**
