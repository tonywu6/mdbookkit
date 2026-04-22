[parallel]
default: prettier stylelint tsc clippy rustfmt

prettier:
    pnpm exec prettier --log-level warn --check "**/*.{ts,js,css,json,jsonc,md}"

stylelint:
    pnpm exec stylelint "**/*.css"

tsc:
    pnpm exec tsc -b

clippy:
    cargo clippy --workspace --all-targets

rustfmt:
    cargo fmt --check --all
