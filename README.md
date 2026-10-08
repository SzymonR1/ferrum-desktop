# Ferrum

Ferrum is a native Rust desktop application for managing existing
Docker-compatible container engines. It does not bundle an engine, install
privileged services, launch an external terminal, or use Electron or Node.js.

## Current status

The initial implementation contains a native eframe window and an asynchronous,
read-only local Docker Engine inventory. The live smoke test returned the same
15-container count as `docker ps -aq`. An X11-backend screenshot captured via
XWayland showed actual container names, images, states, and statuses. A
no-engine X11 launch showed a connection error rather than an empty list.

The default app launch on this Wayland-configured host also connected and
loaded the same inventory, but the local desktop inspection provider does not
support native accessibility or screenshots, so the Wayland surface itself
has not been visually verified. See the [platform support matrix](docs/src/platform-support.md)
for evidence and limitations. This is not a release or platform-support claim.

## Build and run

```sh
cargo build --workspace
cargo run -p ferrum-app
```

The app uses `DOCKER_HOST` when it selects a local Unix socket or named pipe.
Without it, Unix builds try `$XDG_RUNTIME_DIR/docker.sock` and
`/var/run/docker.sock`. Remote TCP, HTTP, TLS, SSH, Docker contexts, and Podman
compatibility are not enabled in this first slice.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo doc --workspace --no-deps
mdbook build docs
```

A read-only live-engine test is available with
`cargo test -p ferrum-engine --test live_engine -- --ignored`. It requires a
local Docker-compatible engine and at least one existing container; it does
not create or modify resources.

## Documentation

- [Build and testing guide](docs/src/developer-guide/building.md)
- [Architecture](docs/src/architecture/overview.md)
- [Security threat model](docs/security/threat-model.md)
- [Platform support](docs/src/platform-support.md)
- [Architecture decisions](docs/decisions/)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

No release artifacts or installers have been published.