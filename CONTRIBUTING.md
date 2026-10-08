# Contributing to Ferrum

## Development setup

Install the stable Rust toolchain and Linux native GUI development libraries. See the [build guide](docs/src/developer-guide/building.md). This slice implements Linux only; Windows/macOS work is deferred to the cross-platform milestone.

## Workflow

1. Find or create a GitHub issue with requirements and acceptance criteria.
2. Create a branch named `feat/<issue>-description`, `fix/<issue>-description`, or another prefix from the repository workflow.
3. Implement a focused vertical slice with behavior tests and documentation.
4. Run formatting, Clippy, tests, build, Rustdoc, and the mdBook build.
5. Open a pull request linked to the issue. Do not claim platform support without runtime evidence.

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

A read-only live engine smoke test is available with `cargo test -p ferrum-engine --test live_engine -- --ignored`; it requires an accessible local Docker-compatible engine and does not mutate resources.
