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

The default suite does not require a host engine. Ignored live-engine tests for
version, inventory, and Docker CLI contexts are read-only. The general
local-engine inventory test may inspect an existing container; these tests do
not mutate resources.

The lifecycle integration test is a separate opt-in, ignored test and mutates
the engine by creating one disposable container and exercising its lifecycle.
It requires `FERRUM_TEST_IMAGE` to name an image already present locally that
contains `/bin/sleep`. The test never pulls an image. A manual prerequisite, if
needed, is to pull a suitable image yourself, for example:

```sh
docker pull busybox:1.37.0
```

Run only the exact ignored lifecycle test (the default suite skips it):

```sh
FERRUM_TEST_IMAGE=busybox:1.37.0 cargo test -p ferrum-engine --test live_engine local_engine_lifecycle_actions_emit_events_and_update_state -- --exact --ignored
```

The test creates one uniquely named container with `--pull never`, `--network none`, no host mounts, and no published ports. It uses the full ID from the Docker create response/cidfile as its only cleanup authority. On failure, best-effort cleanup may force-remove only that validated ID; it never removes volumes. The successful EngineClient Remove action is exercised after stopping the container and does not force-remove or delete volumes. Run only against a disposable local engine.

The engine integration targets Linux local Unix sockets. Podman/rootless
behavior remains unvalidated, remote transports remain unsupported, and Windows
named pipes and macOS remain deferred to issue #21/M8.
