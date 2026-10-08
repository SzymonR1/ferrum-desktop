# Ferrum

Ferrum is a native Rust desktop application for managing existing
Docker-compatible container engines. It does not bundle an engine, install
privileged services, launch an external terminal, or use Electron or Node.js.

## Current status

The Linux local-engine implementation provides container inventory, filtering,
sorting, inspection, historical and live container logs, lifecycle actions, and
external-change synchronization. Actions are offered only for safe known
container states; Remove requires confirmation. Container-only event updates
use bounded delivery and report when synchronization is unavailable, while
manual refresh remains available. Logs are plain text, bounded in memory, and
stream only from the selected local Unix socket.

An X11-backend screenshot captured via XWayland showed actual container names,
images, states, and statuses. A no-engine X11 launch showed a connection error
rather than an empty list. The default app launch on this Wayland-configured
host connected and loaded inventory, but the local desktop inspection provider
does not support native accessibility or screenshots, so the Wayland surface
itself has not been visually verified. UI lifecycle action clicks have not
been visually validated. See the [platform support matrix](docs/src/platform-support.md)
for evidence and limitations. This is not a release or platform-support claim.

## Build and run

```sh
cargo build --workspace
cargo run -p ferrum-app
```

The engine adapter targets Linux local Unix sockets. Startup precedence is an
explicit `DOCKER_CONTEXT` (a local Unix-socket Docker CLI context), a local
`DOCKER_HOST`, the current non-default Docker CLI context, then
`$XDG_RUNTIME_DIR/docker.sock` and `/var/run/docker.sock`. Context selection
requires Docker CLI. Remote endpoints remain unsupported; Podman/rootless
behavior is unvalidated. Windows and macOS remain deferred to issue #21/M8.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

A read-only live-engine inventory test uses this exact filter:
```sh
cargo test -p ferrum-engine --test live_engine local_engine_returns_a_well_formed_container_inventory -- --exact --ignored
```
It reads version and inventory and may inspect one existing container; it does not mutate resources.

An ignored lifecycle integration test is mutating; see the testing guide for its exact command and prerequisites. It requires a locally present `FERRUM_TEST_IMAGE` containing `/bin/sleep`; tests do not pull images and clean up only the created container by its returned full ID.

## Documentation

- [Build and testing guide](docs/src/developer-guide/building.md)
- [Architecture](docs/src/architecture/overview.md)
- [Security threat model](docs/security/threat-model.md)
- [Platform support](docs/src/platform-support.md)
- [Architecture decisions](docs/decisions/)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

No release artifacts or installers have been published.