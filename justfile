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
    #!/usr/bin/env bash
    set -euo pipefail
    source <(cargo bin -- llvm-cov show-env --sh)
    cargo bin -- llvm-cov clean --workspace
    just test $@
    cargo bin -- llvm-cov report --html

cov-clean:
    #!/usr/bin/env bash
    source <(cargo bin -- llvm-cov show-env --sh)
    cargo bin -- llvm-cov clean --workspace
    find . -name '*.profraw' -delete
