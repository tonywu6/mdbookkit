fmt: fmt-cargo fmt-prettier

fmt-cargo:
    -cargo fmt

fmt-prettier:
    -pnpm exec prettier --write .
