test:
    cargo bin nextest run

cov: cov-clean test cov-report

cov-report:
    cargo bin llvm-cov report --html

cov-clean:
    cargo bin llvm-cov clean --profraw-only

fmt: fmt-cargo fmt-prettier

fmt-cargo:
    -cargo fmt

fmt-prettier:
    -pnpm exec prettier --write .
