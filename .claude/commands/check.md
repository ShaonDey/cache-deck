---
description: Pre-commit check (format, lint, tests, build)
---
Run, in order, and stop at the first failure with a short explanation:
1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test`
4. `cargo build --release`
