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

An additional ignored live test exercises the Docker CLI's current local Unix
context and reads its version and inventory:

```sh
cargo test -p ferrum-engine --test live_engine current_local_docker_context_connects_and_returns_a_valid_inventory -- --ignored
```

This test requires an installed Docker CLI, a current context backed by an
accessible local Unix socket, and performs no resource mutations. Context
selection does not validate Podman/rootless behavior or remote TLS/SSH policy;
those remain open in [issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10).
Windows named pipes and macOS remain deferred to issue #21/M8.
