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

The default suite does not require a host engine. Ignored live tests read engine
version and inventory; the general local-engine test also inspects one existing
container without mutating resources. Run it only on a host with an accessible
Docker-compatible engine and at least one container:

```sh
cargo test -p ferrum-engine --test live_engine local_engine_returns_a_well_formed_container_inventory -- --ignored
```

An ignored integration test also connects to the Docker CLI's current local
Unix context:

```sh
cargo test -p ferrum-engine --test live_engine current_local_docker_context_connects_and_returns_a_valid_inventory -- --exact --ignored
```

To verify selection of a different, non-current context, set
`FERRUM_TEST_DOCKER_CONTEXT` to an existing local Unix-socket context name:

```sh
FERRUM_TEST_DOCKER_CONTEXT=my-local-context cargo test -p ferrum-engine --test live_engine named_noncurrent_local_docker_context_connects_and_returns_a_valid_inventory -- --exact --ignored
```

The Docker-context tests require the Docker CLI; the non-current test also
requires that its named context be local and reachable. Podman/rootless behavior
and remote TLS/SSH policy remain open in
[issue #10](https://github.com/SzymonR1/ferrum-desktop/issues/10). Windows named
pipes and macOS remain deferred to issue #21/M8.
