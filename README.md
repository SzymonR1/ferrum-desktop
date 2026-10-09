# Ferrum

Ferrum is a native Rust desktop application for managing existing
Docker-compatible container engines. It does not bundle an engine, install
privileged services, or use Electron or Node.js. Its experimental embedded
terminal does not launch an external terminal.

## Current status

The Linux local-engine implementation provides container inventory, filtering,
sorting, inspection, historical and live container logs, lifecycle actions, and
external-change synchronization. An experimental embedded terminal provides an
interactive exec session for an explicitly selected container. Terminal access
is limited to the selected local Docker-compatible Unix socket; closing a
session does not stop or remove its container. Terminal output is untrusted:
OSC 52 clipboard and title requests are ignored, and clipboard operations are
user initiated. Terminal contents are never logged. Actions are offered only
for safe known container states; Remove requires confirmation. Container-only
event updates use bounded delivery and report when synchronization is
unavailable, while manual refresh remains available. Logs are plain text,
bounded in memory, and stream only from the selected local Unix socket.

Lifecycle request outcomes also emit minimal structured local trace events with
the action and a validated full container ID when available. `request_error`
means Ferrum did not receive success confirmation; the engine state may still
be changing. Raw errors, endpoints, names, and daemon response bodies are
excluded. These events are not a persistent or tamper-proof audit store.

The **Images** page provides image listing and inspection, anonymous public
pulls, local image tagging, and confirmed local image removal through the Linux
local Docker Unix socket. Every tag request requires explicit confirmation
because Docker Engine v1.56 may reassign an existing `repository:tag`; the
image data remains. Removal confirmation snapshots the selected image ID and
known repository tags and warns that removing the final reference can delete
image data. Removal does not force the engine: it may reject images used by
containers or with other tags. The request sets `noprune=true` to preserve
untagged parent images, omits `force`, and sends no registry credentials or
authentication. Ferrum holds one image-mutation gate for pull, tag, or removal.
**Hide progress** suppresses pull updates but keeps draining the bounded Engine
response, so the gate remains closed until the pull reaches a terminal response.
Leaving Images or switching context hides stale updates without aborting the
reader; closing Ferrum does not guarantee that the daemon stops its pull. A
timeout or request error does not prove that engine state stayed unchanged.
Invalidating the page ignores stale UI results but does not abort or claim to
undo a submitted removal; successful removal refreshes inventory only when
Images remains current. Build and push remain unavailable. This sub-slice is
not a release and issue
[#16](https://github.com/SzymonR1/ferrum-desktop/issues/16) remains open.
Podman/rootless and remote image endpoints remain unsupported or unvalidated;
broader endpoint validation remains tracked by the still-open
[#10](https://github.com/SzymonR1/ferrum-desktop/issues/10). No live image
pull, tag, or removal operation or test that mutates image resources is claimed;
native image-page visual validation is not claimed. See the [Images guide](docs/src/user-guide/images.md).

The **Volumes** page offers read-only listing and inspection of Docker volumes on the selected local Docker Unix socket on Linux. It displays only name, driver, scope, creation time, and host mountpoint as plain text. Docker's list/inspect endpoints do not report usage; Ferrum shows no size or reference counts. Creating, removing, connecting, disconnecting, networking, usage reporting, Podman/rootless, and remote endpoints are not supported. Issue #17 remains open; issue #10 remains open for Podman/rootless and endpoint-matrix work. See the [Volumes guide](docs/src/user-guide/volumes.md).


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
requires Docker CLI. Direct remote `DOCKER_HOST` TCP/HTTP/HTTPS/SSH values and
remote Docker contexts return typed unsupported errors without fallback to local
socket discovery. Ferrum does not read remote TLS/SSH credentials. Podman/rootless
behavior is unvalidated. The terminal does not extend this scope: issue #10
remains open for Podman/rootless and broader endpoint validation. Windows and
macOS remain deferred to issue #21/M8.

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
The ignored local image list/inspect smoke test is read-only and requires
`FERRUM_TEST_IMAGE` to name an image already installed locally; it does not pull
or modify images. Pull, tag, and removal behavior is covered only by fake-socket
contract tests and headless UI tests; no live image mutation is performed. The
ignored read-only volume list/inspect test requires `FERRUM_TEST_VOLUME` to name
an existing local volume and only lists/inspects it; it does not mutate or clean
up resources. See the testing guide for exact commands.


An ignored lifecycle integration test is mutating; see the testing guide for its exact command and prerequisites. It requires a locally present `FERRUM_TEST_IMAGE` containing `/bin/sleep`; tests do not pull images and clean up only the created container by its returned full ID.

## Documentation

- [Build and testing guide](docs/src/developer-guide/building.md)
- [Architecture](docs/src/architecture/overview.md)
- [Security threat model](docs/security/threat-model.md)
- [Platform support](docs/src/platform-support.md)
- [Architecture decisions](docs/decisions/)
- [Embedded terminal guide](docs/src/user-guide/terminal.md)
- [Performance and accessibility evidence](docs/src/performance.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

No release artifacts or installers have been published.