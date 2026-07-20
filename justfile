import "scripts/variables.just"

mod check "scripts/check.justfile"
mod docs

default:
    just --list

[positional-arguments]
test *args:
    cargo bin -- nextest run $@

[positional-arguments]
cov *args:
    cargo bin -- llvm-cov nextest --html $@
