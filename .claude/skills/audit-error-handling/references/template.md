<!-- no need to separate each section with dividers -->

## `env.rs`

### 43: `std::fs::read_to_string`

<!--
  - use the last item in the call stack (where the error originates) for the headings
    headings can be 3rd party APIs if the error is coming from them
  - in this example, the program calls `std::fs::read_to_string` in `env.rs` on line 43
-->

Call stack:

1. [`program::cache::read_from_cache`](link/to/where_function_is_called.rs)
<!-- begin from where the error is either printed or dropped -->
2. [`program::env::locate_project`](link/to/program/cache.rs#where-locate_project-is-called)
3. [`std::fs::read_to_string`](link/to/program/env.rs#where-read_to_string-is-called)
   <!-- end with the origin of the error (including from 3rd-party or std APIs) -->
   <!-- do not drill down into external APIs -->

<!--
  - most recent call last, wrap names in `inline code`
  - each item must have both the fully-qualified module and function name
  - even for 1st-party code, include the full crate name

  link for each function points to where it is invoked within the previous function
  (there will be no link for `main()`), for example:

    main.rs:
  3 fn main() {
  4     foo::bar()
  5 }

  1. `main()`
  2. [`foo::bar`](main.rs#L4)

  if the error causes the program to terminate, indicate so with "1. (terminated)" as the first item
-->

Simulated error message:

```
ERROR span1:span2: module_name: Failed to initialize program

Caused by:
    0: Failed to read config:
    1: Reading from (debug representation of `PathWrapper`)
    2: (std::io::Error)
```

<!--
  - if the error is dropped without being logged, indicate with "(none)";
  - wrap code block in ```fenced code```;
  - you are free to paraphrase if the error representation is not readily apparent
-->

Issues:

- `spans_recommended` This error repeats many contextual information ...
  <!-- include the specific lint id if applicable -->
- (additional observations, if any)

<!-- document an error site even if there are no apparent issues. -->
<!-- in which case simply write "Issues: none" without extra explanations -->
