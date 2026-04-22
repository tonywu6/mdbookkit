import "scripts/variables.just"

mod check "scripts/check.justfile"
mod docs

default:
    just --list

[positional-arguments]
test *args:
    cargo bin nextest run $@

cov: cov-clean-all test cov-report

cov-report:
    cargo bin llvm-cov report --html

cov-clean:
    cargo bin llvm-cov clean --profraw-only

cov-clean-all:
    cargo bin llvm-cov clean
