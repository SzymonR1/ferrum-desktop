# Testing Ferrum

Run the deterministic workspace tests and checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

The engine crate includes an ignored, read-only live smoke test. It requests
engine version information and container inventory and verifies displayed
container fields. It requires at least one existing container; it does not
create, stop, or delete resources:

```sh
cargo test -p ferrum-engine --test live_engine -- --ignored
```

Run that test only when the process has access to a local Docker-compatible
engine and at least one existing container. It uses `DOCKER_HOST` if configured.
CI runs the regular suite without requiring a host engine. Podman compatibility
is not established by this test.
