import "scripts/variables.just"

mod check "scripts/check.justfile"
mod docs

default:
    just --list

[positional-arguments]
test *args:
    cargo bin nextest run $@

[positional-arguments]
test-unit-tests *args:
    cargo bin llvm-cov nextest -E 'kind(lib) or kind(bin)' --no-report $@

[positional-arguments]
test-integration-tests *args:
    cargo bin nextest run -E 'kind(test)' $@

cov: cov-clean-all test-unit-tests test-integration-tests cov-report

cov-report:
    cargo bin llvm-cov report --html

cov-clean:
    cargo bin llvm-cov clean --profraw-only

cov-clean-all:
    cargo bin llvm-cov clean
